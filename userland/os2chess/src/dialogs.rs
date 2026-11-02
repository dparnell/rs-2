//! OS/2 Chess's dialogs and windows (their ids are OS/2 Chess's, which
//! its help table uses): Load Game, Delete Game, Set Players, Set
//! Computer Playing Level, Settings, Game Record, Move Status, Time
//! Elapsed, Keyboard, Valid Moves, Captured Pieces, Rotate Board, Pawn
//! Promotion and Player to Move; and the board's pop-up menus.

use alloc::vec::Vec;

use pmkit::*;

use crate::engine::*;
use crate::game::{self, Game};
use crate::{app, side_index, trace, IDM_PUT, TITLE};

pub const DLG_DELETE: u16 = 6;
pub const DLG_LOAD: u16 = 1020;
pub const DLG_PLAYERS: u16 = 1110;
pub const DLG_SETTINGS: u16 = 1130;
pub const DLG_KEYBOARD: u16 = 1150;
pub const DLG_TIMER: u16 = 1160;
pub const DLG_STATUS: u16 = 1210;
pub const DLG_RECORD: u16 = 1220;
pub const DLG_VALID: u16 = 1230;
pub const DLG_CAPTURED: u16 = 1250;
pub const DLG_ROTATE: u16 = 1260;
pub const DLG_PRODUCT: u16 = 1550;
pub const DLG_PROMOTE: u16 = 2000;
pub const DLG_LEVEL: u16 = 2300;
pub const DLG_TOMOVE: u16 = 2500;

const ID_GAMES: u32 = 8131;
const ID_WHUMAN: u32 = 1113;
const ID_WCOMPUTER: u32 = 1114;
const ID_WNETWORK: u32 = 1115;
const ID_BHUMAN: u32 = 1116;
const ID_BCOMPUTER: u32 = 1117;
const ID_BNETWORK: u32 = 1118;
const ID_WMORE: u32 = 1122;
const ID_BMORE: u32 = 1125;
const ID_WNAME: u32 = 1111;
const ID_BNAME: u32 = 1112;
const ID_LEVEL0: u32 = 8136;
const ID_LEVELS: [u32; 5] = [8136, 8137, 8138, 8231, 8232];
const ID_BOOK: u32 = 8139;
const ID_WTIME: u32 = 1161;
const ID_BTIME: u32 = 1162;
const ID_SMOVE: u32 = 1212;
const ID_SCOUNT: u32 = 1214;
const ID_STIME: u32 = 1216;
const ID_WNAMEREC: u32 = 1221;
const ID_BNAMEREC: u32 = 1222;
const ID_WMOVES: u32 = 200;
const ID_BMOVES: u32 = 201;
const ID_VALIDLIST: u32 = 203;
const ID_VALIDCOUNT: u32 = 999;
const ID_CAPTURED: u32 = 1251;
const ID_ANGLE: u32 = 8151;
const ID_ANGLETEXT: u32 = 1261;
const ID_DEFAULT: u32 = 4;
const ID_FROM: u32 = 1152;
const ID_TO: u32 = 1153;
const ID_KMOVE: u32 = 1151;
const ID_WARNINGS: u32 = 3612;
const ID_VIEW2D: u32 = 3670;
const ID_VIEW3D: u32 = 3671;
const ID_SCHEME: u32 = 8232;
const ID_SHOWMOVES: u32 = 3619;
const ID_HELP: u32 = 0x3f5;

/// The dialogs' controls (each shows its dialog's help panel).
pub const CONTROLS: [u16; 37] = [
    ID_GAMES as u16, ID_WHUMAN as u16, ID_WCOMPUTER as u16, ID_WNETWORK as u16, ID_BHUMAN as u16, ID_BCOMPUTER as u16, ID_BNETWORK as u16, ID_WMORE as u16, ID_BMORE as u16, ID_WNAME as u16,
    ID_BNAME as u16, 8136, 8137, 8138, 8231, 8232, ID_BOOK as u16, ID_WTIME as u16, ID_BTIME as u16, ID_SMOVE as u16, ID_SCOUNT as u16, ID_STIME as u16, ID_WNAMEREC as u16, ID_BNAMEREC as u16,
    ID_WMOVES as u16, ID_BMOVES as u16, ID_VALIDLIST as u16, ID_VALIDCOUNT as u16, ID_CAPTURED as u16, ID_ANGLE as u16, ID_ANGLETEXT as u16, ID_FROM as u16, ID_TO as u16, ID_WARNINGS as u16,
    ID_VIEW2D as u16, ID_VIEW3D as u16, ID_SHOWMOVES as u16,
];

fn time_text(ms: u32) -> Vec<u8> {
    let s = ms / 1000;
    alloc::format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60).into_bytes()
}

// ---- Pop-up menus ----

static mut POPUP: u32 = 0;
static mut SETUP_SQUARE: u8 = 0;

fn show_popup(items: &[Menu], x: i32, y: i32) {
    let a = app();
    unsafe {
        if POPUP != 0 {
            WinDestroyWindow(POPUP);
        }
        POPUP = popup_menu(a.client, items);
        WinPopupMenu(a.client, a.client, POPUP, x, y, 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2 | PU_KEYBOARD);
    }
}

/// The board's pop-up menu (OS/2 Chess's menu 1001).
pub fn board_menu(x: i32, y: i32) {
    let a = app();
    use crate::*;
    show_popup(
        &[
            Menu::Sub(IDM_GAME, b"~Game", alloc::vec![Menu::Item(IDM_NEW, b"~New"), Menu::Item(IDM_SAVE, b"~Save"), Menu::Item(IDM_PRINT, b"~Print")]),
            Menu::Sub(IDM_MOVES, b"~Moves", alloc::vec![
                if a.game.moves.is_empty() { Menu::Disabled(IDM_TAKEBACK, b"Take ~back") } else { Menu::Item(IDM_TAKEBACK, b"Take ~back") },
                Menu::Sep,
                Menu::Item(IDM_STATUS, b"~Move status..."),
                Menu::Item(IDM_RECORD, b"~Game record..."),
                Menu::Item(IDM_VALID, b"~Valid moves..."),
                Menu::Item(IDM_CAPTURED, b"~Captured pieces..."),
            ]),
            Menu::Sep,
            Menu::Check(IDM_LABEL, b"~Label board", a.view.labels),
            Menu::Check(IDM_SHOWMOVES, b"~Show possible moves", a.show_moves),
        ],
        x,
        y,
    );
}

/// Set position: the pieces to put on a square.
pub fn piece_menu(sq: u8, x: i32, y: i32) {
    unsafe { SETUP_SQUARE = sq };
    let id = |p: i8| IDM_PUT + (p + 6) as u16;
    show_popup(
        &[
            Menu::Item(id(KING), b"White King"),
            Menu::Item(id(QUEEN), b"White Queen"),
            Menu::Item(id(ROOK), b"White Rook"),
            Menu::Item(id(BISHOP), b"White Bishop"),
            Menu::Item(id(KNIGHT), b"White Knight"),
            Menu::Item(id(PAWN), b"White Pawn"),
            Menu::Sep,
            Menu::Item(id(-KING), b"Black King"),
            Menu::Item(id(-QUEEN), b"Black Queen"),
            Menu::Item(id(-ROOK), b"Black Rook"),
            Menu::Item(id(-BISHOP), b"Black Bishop"),
            Menu::Item(id(-KNIGHT), b"Black Knight"),
            Menu::Item(id(-PAWN), b"Black Pawn"),
            Menu::Sep,
            Menu::Item(id(0), b"~Empty"),
        ],
        x,
        y,
    );
}

pub fn put_piece(p: i8) {
    let a = app();
    let sq = unsafe { SETUP_SQUARE };
    let old = a.game.pos.sq[sq as usize];
    if old.abs() == KING && p != old {
        message(a.frame, b"King cannot be removed from board.", TITLE, MB_OK | MB_WARNING);
        return;
    }
    if p.abs() == PAWN && (rank(sq) == 0 || rank(sq) == 7) {
        unsafe { WinAlarm(HWND_DESKTOP, 0) };
        return;
    }
    if p.abs() == KING {
        // One king of a colour: it moves here.
        let k = a.game.pos.king(p.signum());
        if k < 64 {
            a.game.pos.sq[k as usize] = 0;
        }
    }
    a.game.pos.sq[sq as usize] = p;
    if p.abs() == KING {
        let other = a.game.pos.king(-p.signum());
        if other < 64 && (file(other) - file(sq)).abs() <= 1 && (rank(other) - rank(sq)).abs() <= 1 {
            a.game.pos.sq[sq as usize] = old;
            let k = a.game.pos.king(p.signum());
            let _ = k;
            message(a.frame, b"Kings cannot be next to each other.", TITLE, MB_OK | MB_WARNING);
        }
    }
    invalidate(a.client);
}

/// Options > Set position: start or end setting up the board.
pub fn set_position() {
    let a = app();
    if !a.setting {
        a.request = a.request.wrapping_add(1);
        game::abort_thinking();
        a.setting = true;
        a.selected = None;
        crate::next_turn();
        return;
    }
    let mut p = a.game.pos.clone();
    if p.king(WHITE) >= 64 || p.king(BLACK) >= 64 {
        message(a.frame, b"King cannot be removed from board.", TITLE, MB_OK | MB_WARNING);
        return;
    }
    p.side = to_move();
    p.ep = 64;
    p.halfmove = 0;
    p.fullmove = 1;
    p.infer_castling();
    if p.in_check(-p.side) {
        message(a.frame, b"Invalid move: King is in check.", TITLE, MB_OK | MB_WARNING);
        return;
    }
    let fen = p.fen();
    trace(&[&b"position "[..], &fen].concat());
    a.game = Game::from(&fen);
    a.setting = false;
    a.elapsed = [0; 2];
    crate::next_turn();
}

extern "C" fn tomove_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_COMMAND => {
            unsafe { WinDismissDlg(h, lo(mp1)) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Player to Move: which side moves first in the set-up position.
fn to_move() -> i8 {
    let tpl = template(
        dialog_frame(b"Player to Move", DLG_TOMOVE, 142, 56),
        alloc::vec![label(b"Select side to move first:", (8, 39, 132, 8), DT_LEFT), button(b"~White", (26, 10, 40), 6, BS_DEFAULT), button(b"~Black", (78, 10, 40), 12, 0)],
    );
    if run_dialog(app().frame, tomove_proc, &tpl, 0) == 12 { BLACK } else { WHITE }
}

// ---- Pawn Promotion ----

extern "C" fn promote_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_COMMAND => {
            let id = lo(mp1);
            unsafe { WinDismissDlg(h, if id == DID_CANCEL { 25 } else { id }) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// The piece a pawn becomes (OS/2 Chess's button ids 25 Queen, 24 Rook,
/// 23 Bishop, 22 Knight).
pub fn promotion() -> i8 {
    let tpl = template(
        dialog_frame(b"Pawn Promotion", DLG_PROMOTE, 190, 56),
        alloc::vec![
            label(b"Select piece to promote to:", (8, 39, 170, 8), DT_LEFT),
            button(b"~Queen", (8, 10, 40), 25, BS_DEFAULT),
            button(b"~Rook", (52, 10, 40), 24, 0),
            button(b"~Bishop", (96, 10, 40), 23, 0),
            button(b"~Knight", (140, 10, 40), 22, 0),
        ],
    );
    match run_dialog(app().frame, promote_proc, &tpl, 0) {
        24 => ROOK,
        23 => BISHOP,
        22 => KNIGHT,
        _ => QUEEN,
    }
}

// ---- Load Game and Delete Game ----

static mut GAMES: Vec<Vec<u8>> = Vec::new();

extern "C" fn load_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let files = unsafe { &*core::ptr::addr_of!(GAMES) };
    match m {
        WM_INITDLG => {
            for f in files {
                let mut t = f.clone();
                if let Some(d) = game::describe(f) {
                    t.extend_from_slice(b"   ");
                    t.extend_from_slice(&d);
                }
                lb_add(h, ID_GAMES, &t);
            }
            if !files.is_empty() {
                dlg_msg(h, ID_GAMES, LM_SELECTITEM, 0, 1);
            }
            0
        }
        WM_CONTROL if lo(mp1) == ID_GAMES && hi(mp1) == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            let id = lo(mp1);
            let sel = dlg_msg(h, ID_GAMES, LM_QUERYSELECTION, LIT_FIRST, 0);
            unsafe { WinDismissDlg(h, if id == DID_OK && (sel as usize) < files.len() { 100 + sel } else { DID_CANCEL }) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Game > Load, Game > Delete: pick a saved game.
pub fn load_dialog(delete: bool) {
    let a = app();
    let files = game::saved_games();
    if files.is_empty() {
        message(a.frame, b"There are no saved games in this directory.", TITLE, MB_OK | MB_INFORMATION);
        return;
    }
    unsafe { GAMES = files };
    let (title, id, ok): (&[u8], u16, &[u8]) = if delete { (b"Delete Game", DLG_DELETE, b"~Delete") } else { (b"Load Game", DLG_LOAD, b"~OK") };
    let tpl = template(
        dialog_frame(title, id, 230, 100),
        alloc::vec![
            label(b"Select game:", (12, 86, 120, 8), DT_LEFT),
            list_box((12, 24, 206, 60), ID_GAMES, 0),
            button(ok, (12, 4, 40), DID_OK, BS_DEFAULT),
            button(b"Cancel", (58, 4, 40), DID_CANCEL, 0),
            help_button((104, 4, 40), ID_HELP),
        ],
    );
    let r = run_dialog(a.frame, load_proc, &tpl, 0);
    if r < 100 {
        return;
    }
    let file = unsafe { (&*core::ptr::addr_of!(GAMES))[(r - 100) as usize].clone() };
    if delete {
        if message(a.frame, b"Are you sure?", title, MB_YESNO | MB_ICONQUESTION) == MBID_YES {
            let f = z(&file);
            unsafe { os2::DosDelete(f.as_ptr()) };
            if a.game.file.as_deref() == Some(&file[..]) {
                a.game.file = None;
            }
            trace(&[&b"deleted "[..], &file].concat());
        }
        return;
    }
    if !game::load_game(&file) {
        message(a.frame, b"The file is not an OS/2 Chess game.", TITLE, MB_OK | MB_ERROR);
        return;
    }
    a.elapsed = [0; 2];
    crate::next_turn();
}

// ---- Set Players and the playing level ----

static mut LEVEL_SIDE: usize = 0;

extern "C" fn level_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    let side = unsafe { LEVEL_SIDE };
    match m {
        WM_INITDLG => {
            set_check(h, ID_LEVELS[a.players[side].level as usize % 5], true);
            set_check(h, ID_BOOK, a.book);
            unsafe { WinSetFocus(HWND_DESKTOP, item(h, DID_OK)) };
            1
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == DID_OK {
                if let Some(i) = ID_LEVELS.iter().position(|&l| checked(h, l)) {
                    a.players[side].level = i as u32;
                }
                a.book = checked(h, ID_BOOK);
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Set Computer Playing Level for a side.
fn level(owner: u32, side: usize) {
    unsafe { LEVEL_SIDE = side };
    let mut v = alloc::vec![group_box(b"", (8, 28, 138, 86))];
    for (i, name) in game::LEVEL_NAMES.iter().enumerate() {
        let mut label = b"~".to_vec();
        label.extend_from_slice(name);
        let mut r = radio(&label, (15, 96 - 11 * i as i16, 120, 10), ID_LEVELS[i]);
        if i == 0 {
            r = r.group();
        }
        v.push(r);
    }
    v.push(check_box(b"Use book openings", (15, 34, 128, 10), ID_BOOK).group());
    v.push(button(b"~OK", (8, 6, 36), DID_OK, BS_DEFAULT).group());
    v.push(button(b"Cancel", (51, 6, 40), DID_CANCEL, 0));
    v.push(help_button((100, 6, 40), ID_HELP));
    let _ = ID_LEVEL0;
    let tpl = template(dialog_frame(b"Set Computer Playing Level", DLG_LEVEL, 152, 120), v);
    run_dialog(owner, level_proc, &tpl, 0);
}

extern "C" fn players_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            for (i, (hu, co, nw, name)) in [(ID_WHUMAN, ID_WCOMPUTER, ID_WNETWORK, ID_WNAME), (ID_BHUMAN, ID_BCOMPUTER, ID_BNETWORK, ID_BNAME)].into_iter().enumerate() {
                set_check(h, if a.players[i].computer { co } else { hu }, true);
                enable(h, nw, false);
                dlg_msg(h, name, EM_SETTEXTLIMIT, 31, 0);
                set_item_text(h, name, &a.players[i].name);
            }
            unsafe { WinSetFocus(HWND_DESKTOP, item(h, DID_OK)) };
            1
        }
        WM_COMMAND => {
            let id = lo(mp1);
            match id {
                ID_WMORE => level(h, 0),
                ID_BMORE => level(h, 1),
                _ => {
                    if id == DID_OK {
                        for (i, (co, name)) in [(ID_WCOMPUTER, ID_WNAME), (ID_BCOMPUTER, ID_BNAME)].into_iter().enumerate() {
                            a.players[i].computer = checked(h, co);
                            let n = item_text(h, name);
                            a.players[i].name = if n.is_empty() {
                                if a.players[i].computer { b"Computer".to_vec() } else { game::side_name(if i == 0 { WHITE } else { BLACK }).to_vec() }
                            } else {
                                n
                            };
                        }
                    }
                    unsafe { WinDismissDlg(h, id) };
                }
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Options > Set players (also shown when OS/2 Chess starts).
pub fn players() {
    let a = app();
    let side = |x: i16, title: &[u8], ids: (u32, u32, u32, u32, u32), keys: [&'static [u8]; 3]| -> Vec<Item> {
        let (hu, co, nw, more, name) = ids;
        alloc::vec![
            group_box(title, (x, 28, 112, 82)),
            radio(keys[0], (x + 6, 86, 62, 10), hu).group(),
            radio(keys[1], (x + 6, 68, 62, 10), co),
            radio(keys[2], (x + 6, 50, 62, 10), nw),
            button(b"~More...", (x + 68, 66, 38), more, 0).group(),
            label(b"Name:", (x + 4, 36, 28, 8), DT_LEFT),
            entry((x + 40, 36, 64, 8), name, 0),
        ]
    };
    let mut v = side(8, b"White Side", (ID_WHUMAN, ID_WCOMPUTER, ID_WNETWORK, ID_WMORE, ID_WNAME), [b"~Human", b"~Computer", b"~Network"]);
    v.extend(side(132, b"Black Side", (ID_BHUMAN, ID_BCOMPUTER, ID_BNETWORK, ID_BMORE, ID_BNAME), [b"H~uman", b"Com~puter", b"N~etwork"]));
    v.push(button(b"~OK", (10, 8, 36), DID_OK, BS_DEFAULT).group());
    v.push(button(b"Cancel", (52, 8, 40), DID_CANCEL, 0));
    v.push(help_button((98, 8, 40), ID_HELP));
    let tpl = template(dialog_frame(b"Set Players", DLG_PLAYERS, 256, 120), v);
    if run_dialog(a.frame, players_proc, &tpl, 0) == DID_OK {
        trace(alloc::format!("players: white {} black {}", if a.players[0].computer { "computer" } else { "human" }, if a.players[1].computer { "computer" } else { "human" }).as_bytes());
        if a.thinking && !a.players[side_index(a.game.pos.side)].computer {
            a.request = a.request.wrapping_add(1);
            game::abort_thinking();
        }
        refresh_record();
        if !a.thinking {
            crate::next_turn();
        }
    }
}

// ---- Settings ----

extern "C" fn settings_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            set_check(h, ID_WARNINGS, a.warnings);
            set_check(h, ID_SHOWMOVES, a.show_moves);
            set_check(h, if a.view.three_d { ID_VIEW3D } else { ID_VIEW2D }, true);
            for (name, _) in crate::draw::SCHEMES.iter() {
                lb_add(h, ID_SCHEME, name);
            }
            dlg_msg(h, ID_SCHEME, LM_SELECTITEM, a.scheme as u32, 1);
            0
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == DID_OK {
                a.warnings = checked(h, ID_WARNINGS);
                a.show_moves = checked(h, ID_SHOWMOVES);
                a.view.three_d = checked(h, ID_VIEW3D);
                let s = dlg_msg(h, ID_SCHEME, LM_QUERYSELECTION, LIT_FIRST, 0) as usize;
                if s < crate::draw::SCHEMES.len() {
                    a.scheme = s;
                }
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Options > Settings: warnings, the view, the colours.
pub fn settings() {
    let a = app();
    let tpl = template(
        dialog_frame(b"OS/2 Chess Settings", DLG_SETTINGS, 230, 150),
        alloc::vec![
            group_box(b"Options", (6, 92, 128, 52)),
            check_box(b"Warning messages", (12, 120, 118, 10), ID_WARNINGS),
            check_box(b"Show possible moves", (12, 106, 118, 10), ID_SHOWMOVES),
            group_box(b"View", (140, 92, 84, 52)),
            radio(b"~2-Dimensional", (146, 120, 74, 10), ID_VIEW2D).group(),
            radio(b"~3-Dimensional", (146, 106, 74, 10), ID_VIEW3D),
            group_box(b"Colors", (6, 26, 218, 62)),
            list_box((12, 32, 206, 46), ID_SCHEME, 0).group(),
            button(b"~OK", (8, 6, 40), DID_OK, BS_DEFAULT).group(),
            button(b"Cancel", (54, 6, 40), DID_CANCEL, 0),
            help_button((100, 6, 40), ID_HELP),
        ],
    );
    if run_dialog(a.frame, settings_proc, &tpl, 0) == DID_OK {
        crate::update_menu();
        invalidate(a.client);
    }
}

// ---- Modeless windows ----

extern "C" fn modeless_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_COMMAND => {
            let id = lo(mp1);
            if h == a.keyboard_dlg && id == DID_OK {
                keyboard_move(h);
                return 0;
            }
            if h == a.record_dlg {
                a.record_dlg = 0;
            } else if h == a.status_dlg {
                a.status_dlg = 0;
            } else if h == a.timer_dlg {
                a.timer_dlg = 0;
            } else if h == a.keyboard_dlg {
                a.keyboard_dlg = 0;
            }
            unsafe { WinDestroyWindow(h) };
            0
        }
        WM_CLOSE => {
            post(h, WM_COMMAND, DID_CANCEL, 0);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn modeless(handle: &mut u32, tpl: Vec<u8>) -> bool {
    let a = app();
    if *handle != 0 {
        unsafe { WinSetFocus(HWND_DESKTOP, *handle) };
        return false;
    }
    *handle = modeless_dialog(a.frame, modeless_proc, &tpl, 0);
    true
}

/// Moves > Game record.
pub fn record_window() {
    let a = app();
    let tpl = template(
        dialog_frame(b"Game Record", DLG_RECORD, 150, 128),
        alloc::vec![
            label(b"White", (10, 116, 60, 8), DT_LEFT),
            label(b"Black", (76, 116, 60, 8), DT_LEFT),
            text_id(b"", (10, 104, 60, 8), ID_WNAMEREC, DT_LEFT),
            text_id(b"", (76, 104, 60, 8), ID_BNAMEREC, DT_LEFT),
            list_box((6, 26, 70, 74), ID_WMOVES, 0),
            list_box((74, 26, 70, 74), ID_BMOVES, 0),
            button(b"Cancel", (10, 6, 40), DID_CANCEL, BS_DEFAULT),
            help_button((56, 6, 40), ID_HELP),
        ],
    );
    let mut h = a.record_dlg;
    if modeless(&mut h, tpl) {
        a.record_dlg = h;
        refresh_record();
    }
}

pub fn refresh_record() {
    let a = app();
    let h = a.record_dlg;
    if h == 0 {
        return;
    }
    set_item_text(h, ID_WNAMEREC, &a.players[0].name);
    set_item_text(h, ID_BNAMEREC, &a.players[1].name);
    for id in [ID_WMOVES, ID_BMOVES] {
        dlg_msg(h, id, LM_DELETEALL, 0, 0);
    }
    // A set-up position with black to move starts in the black column.
    let black_first = a.game.start.split(|&c| c == b' ').nth(1) == Some(b"b");
    for (i, n) in a.game.notes.iter().enumerate() {
        let white = (i % 2 == 0) != black_first;
        let mut t = alloc::format!("{}. ", (i + black_first as usize) / 2 + 1).into_bytes();
        t.extend_from_slice(n);
        lb_add(h, if white { ID_WMOVES } else { ID_BMOVES }, &t);
    }
    for id in [ID_WMOVES, ID_BMOVES] {
        let n = dlg_msg(h, id, LM_QUERYITEMCOUNT, 0, 0);
        if n > 6 {
            dlg_msg(h, id, LM_SETTOPINDEX, n - 6, 0);
        }
    }
}

/// Moves > Move status: what the computer is thinking about.
pub fn status_window() {
    let a = app();
    let tpl = template(
        dialog_frame(b"Move Status", DLG_STATUS, 150, 70),
        alloc::vec![
            text_id(b"Move: ", (6, 55, 138, 8), ID_SMOVE, DT_LEFT),
            label(b"Moves completed", (6, 40, 90, 8), DT_LEFT),
            text_id(b"", (95, 40, 48, 8), ID_SCOUNT, DT_RIGHT),
            label(b"Elapsed time", (6, 25, 84, 8), DT_LEFT),
            text_id(b"", (85, 25, 58, 8), ID_STIME, DT_RIGHT),
            button(b"Cancel", (21, 4, 40), DID_CANCEL, BS_DEFAULT),
            help_button((77, 4, 40), ID_HELP),
        ],
    );
    let mut h = a.status_dlg;
    if modeless(&mut h, tpl) {
        a.status_dlg = h;
        refresh_status(true);
    }
}

pub fn refresh_status(done: bool) {
    let a = app();
    let h = a.status_dlg;
    if h == 0 {
        return;
    }
    let (nodes, depth, _) = unsafe { game::PROGRESS };
    let mut t = b"Move: ".to_vec();
    if a.thinking {
        t.extend_from_slice(b"thinking");
    } else if let Some(n) = a.game.notes.last() {
        t.extend_from_slice(n);
    }
    set_item_text(h, ID_SMOVE, &t);
    let count = if done && !a.thinking { alloc::format!("{nodes} ({depth})") } else { alloc::format!("{}", a.game.moves.len()) };
    set_item_text(h, ID_SCOUNT, count.as_bytes());
    let el = if a.thinking { crate::ms().wrapping_sub(a.think_start) } else { 0 };
    set_item_text(h, ID_STIME, &time_text(el));
}

/// Options > Timer: the time each side has used.
pub fn timer_window() {
    let a = app();
    let tpl = template(
        dialog_frame(b"Time Elapsed", DLG_TIMER, 124, 52),
        alloc::vec![
            label(b"White", (6, 36, 48, 8), DT_CENTER),
            label(b"Black", (68, 36, 48, 8), DT_CENTER),
            text_id(b"", (8, 22, 44, 10), ID_WTIME, DT_CENTER),
            text_id(b"", (70, 22, 44, 10), ID_BTIME, DT_CENTER),
            button(b"Cancel", (17, 4, 40), DID_CANCEL, BS_DEFAULT),
            help_button((67, 4, 40), ID_HELP),
        ],
    );
    let mut h = a.timer_dlg;
    if modeless(&mut h, tpl) {
        a.timer_dlg = h;
        refresh_timer();
    }
}

pub fn refresh_timer() {
    let a = app();
    if a.timer_dlg != 0 {
        set_item_text(a.timer_dlg, ID_WTIME, &time_text(a.elapsed[0]));
        set_item_text(a.timer_dlg, ID_BTIME, &time_text(a.elapsed[1]));
    }
}

/// Options > Keyboard input: moves typed as squares.
pub fn keyboard_window() {
    let a = app();
    let tpl = template(
        dialog_frame(b"Keyboard", DLG_KEYBOARD, 134, 70),
        alloc::vec![
            text_id(b"Move: ", (6, 55, 120, 8), ID_KMOVE, DT_LEFT),
            label(b"From", (10, 38, 58, 8), DT_LEFT),
            label(b"To", (70, 38, 58, 8), DT_LEFT),
            entry((10, 25, 24, 8), ID_FROM, 0),
            entry((70, 25, 24, 8), ID_TO, 0),
            button(b"~Move", (6, 4, 38), DID_OK, BS_DEFAULT),
            button(b"Cancel", (49, 4, 38), DID_CANCEL, 0),
            help_button((92, 4, 38), ID_HELP),
        ],
    );
    let mut h = a.keyboard_dlg;
    if modeless(&mut h, tpl) {
        a.keyboard_dlg = h;
        dlg_msg(h, ID_FROM, EM_SETTEXTLIMIT, 2, 0);
        dlg_msg(h, ID_TO, EM_SETTEXTLIMIT, 2, 0);
        unsafe { WinSetFocus(HWND_DESKTOP, item(h, ID_FROM)) };
    }
}

fn keyboard_move(h: u32) {
    let a = app();
    let (f, t) = (parse_square(&item_text(h, ID_FROM)), parse_square(&item_text(h, ID_TO)));
    match (f, t) {
        (Some(f), Some(t)) => {
            if a.thinking || a.game.over.is_some() || a.players[side_index(a.game.pos.side)].computer {
                unsafe { WinAlarm(HWND_DESKTOP, 0) };
                return;
            }
            let mut m = b"Move: ".to_vec();
            m.extend_from_slice(&square_name(f));
            m.push(b'-');
            m.extend_from_slice(&square_name(t));
            set_item_text(h, ID_KMOVE, &m);
            crate::try_move(f, t);
            set_item_text(h, ID_FROM, b"");
            set_item_text(h, ID_TO, b"");
            unsafe { WinSetFocus(HWND_DESKTOP, item(h, ID_FROM)) };
        }
        _ => crate::invalid(b"Try again..."),
    }
}

// ---- Valid moves, captured pieces, rotating ----

static mut VALID: Vec<Move> = Vec::new();

extern "C" fn valid_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    let moves = unsafe { &*core::ptr::addr_of!(VALID) };
    match m {
        WM_INITDLG => {
            for mv in moves {
                let mut t = game::piece_name(a.game.pos.sq[mv.from as usize]).to_vec();
                t.push(b' ');
                t.extend_from_slice(&a.game.pos.notation(*mv));
                lb_add(h, ID_VALIDLIST, &t);
            }
            set_item_text(h, ID_VALIDCOUNT, alloc::format!("Total moves: {}", moves.len()).as_bytes());
            dlg_msg(h, ID_VALIDLIST, LM_SELECTITEM, 0, 1);
            0
        }
        WM_CONTROL if lo(mp1) == ID_VALIDLIST && hi(mp1) == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            let id = lo(mp1);
            let sel = dlg_msg(h, ID_VALIDLIST, LM_QUERYSELECTION, LIT_FIRST, 0);
            unsafe { WinDismissDlg(h, if id == DID_OK && (sel as usize) < moves.len() { 100 + sel } else { DID_CANCEL }) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Moves > Valid moves: the moves allowed now; Move plays the one chosen.
pub fn valid_moves() {
    let a = app();
    let moves = a.game.pos.legal_moves();
    unsafe { VALID = moves };
    let tpl = template(
        dialog_frame(b"Valid Moves", DLG_VALID, 134, 150),
        alloc::vec![
            list_box((6, 37, 122, 108), ID_VALIDLIST, 0),
            text_id(b"", (6, 24, 120, 8), ID_VALIDCOUNT, DT_LEFT),
            button(b"~Move", (6, 6, 38), DID_OK, BS_DEFAULT),
            button(b"Cancel", (48, 6, 38), DID_CANCEL, 0),
            help_button((90, 6, 38), ID_HELP),
        ],
    );
    let r = run_dialog(a.frame, valid_proc, &tpl, 0);
    let human = !a.players[side_index(a.game.pos.side)].computer;
    if r >= 100 && human && !a.thinking {
        let m = unsafe { (&*core::ptr::addr_of!(VALID))[(r - 100) as usize] };
        crate::play(m);
    }
}

extern "C" fn plain_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_COMMAND => {
            unsafe { WinDismissDlg(h, lo(mp1)) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Moves > Captured pieces.
pub fn captured() {
    let a = app();
    let mut text = Vec::new();
    for (side, title) in [(WHITE, &b"White's pieces taken:"[..]), (BLACK, b"Black's pieces taken:")] {
        text.extend_from_slice(title);
        let mut any = false;
        for kind in [QUEEN, ROOK, BISHOP, KNIGHT, PAWN] {
            let n = a.game.captured.iter().filter(|&&p| p == kind * side).count();
            if n > 0 {
                text.extend_from_slice(alloc::format!(" {} {}{},", n, core::str::from_utf8(game::piece_name(kind)).unwrap_or(""), if n > 1 { "s" } else { "" }).as_bytes());
                any = true;
            }
        }
        if any {
            text.pop();
        } else {
            text.extend_from_slice(b" none");
        }
        text.extend_from_slice(b". ");
    }
    let tpl = template(
        dialog_frame(b"Captured Pieces", DLG_CAPTURED, 140, 96),
        alloc::vec![
            text_id(&text, (6, 30, 128, 60), ID_CAPTURED, DT_LEFT | DT_TOP | DT_WORDBREAK),
            button(b"Cancel", (10, 8, 40), DID_CANCEL, BS_DEFAULT),
            help_button((56, 8, 40), ID_HELP),
        ],
    );
    run_dialog(a.frame, plain_proc, &tpl, 0);
}

extern "C" fn rotate_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    let show = |h: u32, v: i32| {
        set_item_text(h, ID_ANGLETEXT, alloc::format!("{v} degrees").as_bytes());
    };
    match m {
        WM_INITDLG => {
            let sb = item(h, ID_ANGLE);
            send(sb, SBM_SETSCROLLBAR, a.view.angle as u32, mp(0, 359));
            show(h, a.view.angle);
            unsafe { WinSetFocus(HWND_DESKTOP, sb) };
            1
        }
        WM_HSCROLL => {
            let sb = item(h, ID_ANGLE);
            let cur = send(sb, SBM_QUERYPOS, 0, 0) as i32;
            let new = match hi(mp2) {
                SB_LINELEFT => cur - 5,
                SB_LINERIGHT => cur + 5,
                SB_PAGELEFT => cur - 45,
                SB_PAGERIGHT => cur + 45,
                SB_SLIDERTRACK | SB_SLIDERPOSITION => lo(mp2) as i16 as i32,
                _ => return 0,
            }
            .rem_euclid(360);
            send(sb, SBM_SETPOS, new as u32, 0);
            show(h, new);
            a.view.angle = new;
            invalidate(a.client);
            0
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == ID_DEFAULT {
                send(item(h, ID_ANGLE), SBM_SETPOS, 0, 0);
                show(h, 0);
                a.view.angle = 0;
                invalidate(a.client);
                return 0;
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// View > Rotate 3-D board: turn the board around its centre.
pub fn rotate() {
    let a = app();
    let old = a.view.angle;
    let tpl = template(
        dialog_frame(b"Rotate Board", DLG_ROTATE, 196, 80),
        alloc::vec![
            pmkit::tpl::item(WC_SCROLLBAR, b"", WS_VISIBLE | SBS_HORZ | WS_TABSTOP, (9, 26, 178, 10), ID_ANGLE as u16),
            label(b"Clockwise", (10, 38, 60, 8), DT_LEFT),
            label(b"Counter clockwise", (100, 38, 87, 8), DT_RIGHT),
            text_id(b"", (40, 56, 116, 10), ID_ANGLETEXT, DT_CENTER),
            button(b"OK", (10, 6, 40), DID_OK, BS_DEFAULT),
            button(b"~Default", (54, 6, 40), ID_DEFAULT, 0),
            button(b"Cancel", (98, 6, 40), DID_CANCEL, 0),
            help_button((142, 6, 40), ID_HELP),
        ],
    );
    if run_dialog(a.frame, rotate_proc, &tpl, 0) != DID_OK {
        a.view.angle = old;
    }
    invalidate(a.client);
}
