//! KLONDIKE.EXE: rs2's Klondike Solitaire, a re-implementation of OS/2's.
//!
//! As OS/2's (studied in its binary and resources): the menus Game (New
//! deal, Auto play), Moves (Take back, Replay, Cheat, Auto finish),
//! Options (Timer, Score On/Off/Reset, Color scheme Spring/Summer/
//! Autumn/Winter, Settings..., Save settings) and Help, with OS/2's
//! command ids, accelerators (Ctrl+N, P, T, R, C, F; Alt+Backspace) and
//! help panel numbers; the four game variations (draw 3 with no, 3 or
//! any number of turns of the talon, draw 1); Vegas-style scoring (see
//! game.rs); a Settings notebook (Options, Card backs); the settings in
//! KLONDIKE.INI, application PATIENCE, as OS/2's keeps them, asked
//! about when changed and not saved at the end. Cards are dragged with
//! mouse button 1 (or, with Drag cards off, clicked and then the place
//! clicked), a double click sends a card to its foundation, and the
//! keyboard plays too (cursor keys, Space or Enter, Esc).
//!
//! The cards are rs2's own, drawn with GPI (cards.rs); there is no
//! opening video and there are no sounds.
//!
//! `KLONDIKE /DEAL n` deals from seed n (for tests).
//!
//! `KLONDIKE /SELFTEST` checks the game logic and the card drawing and
//! prints PASS/FAIL lines and "klondike selftest done: N failure(s)".

#![no_std]
#![no_main]

extern crate alloc;

mod cards;
mod game;
mod selftest;
mod settings;

use alloc::vec::Vec;

use pmkit::gpi::{self, MemPs};
use pmkit::help::{Help, HelpWindow};
use pmkit::*;

use cards::Atlas;
use game::{Game, Move, FOUNDATION, PILES, TABLEAU, TALON, WASTE};
use settings::Settings;

os2::entry!(main);

// Menu commands (OS/2's ids).
const IDM_GAME: u16 = 1150;
const IDM_NEWDEAL: u16 = 1151;
const IDM_AUTOPLAY: u16 = 1153;
const IDM_MOVES: u16 = 1200;
const IDM_TAKEBACK: u16 = 1201;
const IDM_REPLAY: u16 = 1202;
const IDM_CHEAT: u16 = 1203;
const IDM_AUTOFINISH: u16 = 1204;
const IDM_OPTIONS: u16 = 1300;
const IDM_COLORS: u16 = 1301;
const IDM_SCORE: u16 = 1320;
const IDM_SCOREON: u16 = 1321;
const IDM_SCOREOFF: u16 = 1322;
const IDM_SCORERESET: u16 = 1323;
const IDM_TIMER: u16 = 1330;
const IDM_SETTINGS: u16 = 3990;
const IDM_SAVE: u16 = 1500;
const IDM_HELP: u16 = 64022;
const IDM_USINGHELP: u16 = 64023;
const IDM_GENERALHELP: u16 = 64024;
const IDM_KEYSHELP: u16 = 64025;
const IDM_HELPINDEX: u16 = 64026;
const IDM_PRODUCT: u16 = 64096;

const FRAME_ID: u32 = 1000;
const DLG_PRODUCT: u16 = 3500;
/// The keys help panel.
const PANEL_KEYS: u16 = 1120;
const HELP_FILE: &[u8] = b"KLONDIKE.HLP";
const TITLE: &[u8] = b"Klondike Solitaire";

const TID_CLOCK: u32 = 1;
const TID_AUTO: u32 = 2;
/// Posted after a move that won the game.
const WM_WON: u32 = WM_USER + 1;

/// Where everything is on the board (pels, origin bottom left).
#[derive(Default, Clone, Copy)]
struct Layout {
    w: i32,
    h: i32,
    cw: i32,
    ch: i32,
    gap: i32,
    /// The bottom of the top row's cards.
    top_y: i32,
    /// The top of the tableau's first cards.
    tab_top: i32,
    status_h: i32,
    down_off: i32,
    up_off: [i32; 7],
}

struct Drag {
    from: usize,
    n: usize,
    /// The pointer's distance from the picked-up card's bottom left.
    dx: i32,
    dy: i32,
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Auto {
    Off,
    Finish,
    Play { idle: usize },
}

pub struct App {
    pub hab: u32,
    pub frame: u32,
    client: u32,
    menu: u32,
    help: Option<&'static mut Help>,
    game: Game,
    pub settings: Settings,
    /// The settings as saved (asked about at the end when different).
    saved: Settings,
    pub atlas: Option<Atlas>,
    back: Option<MemPs>,
    lay: Layout,
    drag: Option<Drag>,
    /// Clicked cards waiting for their place (pile, count).
    sel: Option<(usize, usize)>,
    /// The keyboard's pile and whether it is shown.
    cursor: usize,
    show_cursor: bool,
    secs: u32,
    running: bool,
    auto: Auto,
    cheat: bool,
    cheat_turns: u32,
    rng: Option<Rng>,
}

static mut APP: App = App {
    hab: 0,
    frame: 0,
    client: 0,
    menu: 0,
    help: None,
    game: Game {
        piles: [const { Vec::new() }; PILES],
        draw: 3,
        turns_allowed: game::MANY,
        turns: 0,
        auto_flip: true,
        score: game::START_SCORE,
        cheats: 0,
        history: Vec::new(),
        done: 0,
    },
    settings: settings::DEFAULTS,
    saved: settings::DEFAULTS,
    atlas: None,
    back: None,
    lay: Layout { w: 0, h: 0, cw: 0, ch: 0, gap: 0, top_y: 0, tab_top: 0, status_h: 0, down_off: 0, up_off: [0; 7] },
    drag: None,
    sel: None,
    cursor: TABLEAU,
    show_cursor: false,
    secs: 0,
    running: false,
    auto: Auto::Off,
    cheat: false,
    cheat_turns: 0,
    rng: None,
};

pub fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

pub fn trace(s: &[u8]) {
    pmkit::trace("klondike", s);
}

fn main(s: &os2::Startup) -> u32 {
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        return 1;
    }
    if arg(s).eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    let (st, pos) = settings::load();
    a.settings = st;
    a.saved = st;
    // `/DEAL n`: deals from a fixed seed (for tests).
    let w = words(arg(s));
    a.rng = Some(match w.first() {
        Some(f) if f.eq_ignore_ascii_case(b"/DEAL") => Rng::seeded(w.get(1).and_then(|n| parse_num(n)).unwrap_or(1) as u32),
        _ => Rng::new(),
    });
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(a.hab, b"Patience", client_proc, CS_SIZEREDRAW, 0, fcf, TITLE, FRAME_ID);
    if frame == 0 {
        return 1;
    }
    a.frame = frame;
    a.client = client;
    a.menu = set_menu(frame, &menu_items());
    let (c, cs) = (AF_CHAR | AF_CONTROL, AF_CHAR | AF_CONTROL);
    set_accels(
        a.hab,
        frame,
        &[
            (c, b'c' as u16, IDM_CHEAT),
            (cs, b'C' as u16, IDM_CHEAT),
            (c, b'f' as u16, IDM_AUTOFINISH),
            (cs, b'F' as u16, IDM_AUTOFINISH),
            (c, b'n' as u16, IDM_NEWDEAL),
            (cs, b'N' as u16, IDM_NEWDEAL),
            (c, b'p' as u16, IDM_AUTOPLAY),
            (cs, b'P' as u16, IDM_AUTOPLAY),
            (c, b'r' as u16, IDM_REPLAY),
            (cs, b'R' as u16, IDM_REPLAY),
            (c, b't' as u16, IDM_TAKEBACK),
            (cs, b'T' as u16, IDM_TAKEBACK),
            (AF_VIRTUALKEY | AF_ALT, VK_BACKSPACE, IDM_TAKEBACK),
        ],
    );
    a.help = Help::create(a.hab, frame, HELP_FILE, b"Klondike Solitaire Help", &help_windows());
    apply_rules();
    a.game.deal(&game::shuffled(a.rng.as_mut().unwrap()));
    trace(b"dealt");
    update_menu();
    let (sw, sh) = screen();
    let (x, y, cx, cy) = match pos {
        Some([x, y, cx, cy]) if cx > 100 && cy > 100 => (x.clamp(-cx / 2, sw - 32), y.clamp(0, sh - 32), cx, cy),
        _ => {
            let (cx, cy) = (640.min(sw), 480.min(sh));
            ((sw - cx) / 2, (sh - cy) / 2, cx, cy)
        }
    };
    unsafe {
        WinSetWindowPos(frame, HWND_TOP, x, y, cx, cy, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, client);
        WinStartTimer(a.hab, client, TID_CLOCK, 1000);
    }
    message_loop(a.hab);
    let swp = window_pos(frame);
    if swp.fl & (SWP_MINIMIZE | SWP_MAXIMIZE) == 0 && swp.cx > 0 {
        settings::save_position(&swp);
    }
    unsafe {
        if let Some(h) = a.help.as_mut() {
            h.destroy(frame);
        }
        a.back = None;
        a.atlas = None;
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

fn menu_items() -> Vec<Menu<'static>> {
    alloc::vec![
        Menu::Sub(IDM_GAME, b"~Game", alloc::vec![Menu::Item(IDM_NEWDEAL, b"~New deal\tCtrl+N"), Menu::Item(IDM_AUTOPLAY, b"Auto ~play\tCtrl+P")]),
        Menu::Sub(
            IDM_MOVES,
            b"~Moves",
            alloc::vec![
                Menu::Item(IDM_TAKEBACK, b"~Take back\tCtrl+T"),
                Menu::Item(IDM_REPLAY, b"~Replay\tCtrl+R"),
                Menu::Sep,
                Menu::Item(IDM_CHEAT, b"~Cheat\tCtrl+C"),
                Menu::Sep,
                Menu::Item(IDM_AUTOFINISH, b"Auto ~finish\tCtrl+F"),
            ]
        ),
        Menu::Sub(
            IDM_OPTIONS,
            b"~Options",
            alloc::vec![
                Menu::Item(IDM_TIMER, b"~Timer"),
                Menu::Sub(IDM_SCORE, b"Sco~re", alloc::vec![Menu::Item(IDM_SCOREON, b"~On"), Menu::Item(IDM_SCOREOFF, b"O~ff"), Menu::Sep, Menu::Item(IDM_SCORERESET, b"~Reset")]),
                Menu::Sep,
                Menu::Sub(
                    IDM_COLORS,
                    b"~Color scheme",
                    alloc::vec![Menu::Item(1302, b"~Spring"), Menu::Item(1303, b"S~ummer"), Menu::Item(1304, b"~Autumn"), Menu::Item(1305, b"~Winter")]
                ),
                Menu::Sep,
                Menu::Item(IDM_SETTINGS, b"~Settings..."),
                Menu::Item(IDM_SAVE, b"S~ave settings"),
            ]
        ),
        Menu::Sub(
            IDM_HELP,
            b"~Help",
            alloc::vec![
                Menu::Item(IDM_HELPINDEX, b"Help ~index"),
                Menu::Item(IDM_GENERALHELP, b"~General help"),
                Menu::Item(IDM_USINGHELP, b"~Using help"),
                Menu::Item(IDM_KEYSHELP, b"~Keys help"),
                Menu::Sep,
                Menu::Item(IDM_PRODUCT, b"~Product information"),
            ]
        ),
    ]
}

/// The help table: the frame's (menu items: OS/2's subtable) and the
/// Product information window's.
fn help_windows() -> [HelpWindow<'static>; 2] {
    [
        HelpWindow {
            id: FRAME_ID as u16,
            ext: 1000,
            items: &[
                (IDM_GAME, 1150),
                (IDM_NEWDEAL, 1150),
                (IDM_AUTOPLAY, 1150),
                (IDM_MOVES, 1200),
                (IDM_TAKEBACK, 1200),
                (IDM_REPLAY, 1200),
                (IDM_CHEAT, 1200),
                (IDM_AUTOFINISH, 1200),
                (IDM_OPTIONS, 1300),
                (IDM_TIMER, 1330),
                (IDM_SCORE, 1320),
                (IDM_SCOREON, 1320),
                (IDM_SCOREOFF, 1320),
                (IDM_SCORERESET, 1320),
                (IDM_COLORS, 1301),
                (1302, 1301),
                (1303, 1301),
                (1304, 1301),
                (1305, 1301),
                (IDM_SETTINGS, 3990),
                (IDM_SAVE, 1500),
                (IDM_HELP, 64022),
                (IDM_HELPINDEX, 64026),
                (IDM_GENERALHELP, 64024),
                (IDM_USINGHELP, 64023),
                (IDM_KEYSHELP, 64025),
                (IDM_PRODUCT, 64096),
            ],
        },
        HelpWindow { id: DLG_PRODUCT, ext: 3500, items: &[(1, 3500), (0x3f5, 3500)] },
    ]
}

pub fn show_help(panel: u16) {
    match app().help.as_deref() {
        Some(h) => h.show(panel),
        None => help_request(HM_DISPLAY_HELP, 0, 0),
    }
}

fn help_request(msg: u32, mp1: u32, mp2: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, mp1, mp2);
}

/// The game's rules from the settings.
fn apply_rules() {
    let a = app();
    let (draw, turns) = a.settings.rules();
    a.game.draw = draw;
    a.game.turns_allowed = turns;
    a.game.auto_flip = a.settings.auto_flip;
}

/// New settings (from the menus or the notebook): the rules, the
/// pictures and the menus follow.
pub fn apply_settings(s: Settings) {
    let a = app();
    let old = a.settings;
    a.settings = s;
    apply_rules();
    if old.color_set != s.color_set || old.card_back != s.card_back {
        a.atlas = None;
    }
    if !s.drag {
        a.drag = None;
    }
    update_menu();
    redraw();
}

fn update_menu() {
    let a = app();
    let m = a.menu;
    menu_enable(m, IDM_TAKEBACK, a.game.can_take_back());
    menu_enable(m, IDM_REPLAY, a.game.can_replay());
    menu_check(m, IDM_CHEAT, a.cheat);
    menu_check(m, IDM_AUTOPLAY, matches!(a.auto, Auto::Play { .. }));
    menu_check(m, IDM_TIMER, a.settings.timer);
    menu_check(m, IDM_SCOREON, a.settings.score);
    menu_check(m, IDM_SCOREOFF, !a.settings.score);
    for i in 0..4 {
        menu_check(m, 1302 + i, a.settings.color_set == i);
    }
}

// ---- Layout ----

fn layout(w: i32, h: i32) -> Layout {
    let gap = (w / 48).max(4);
    let status_h = 20;
    let mut cw = ((w - 8 * gap) / 7).clamp(24, 140);
    // Two card heights and a fan must fit above the status line.
    let max_ch = (h - status_h - 3 * gap) * 10 / 32;
    if cw * 7 / 5 > max_ch {
        cw = (max_ch * 5 / 7).max(24);
    }
    let ch = cw * 7 / 5;
    let top_y = h - gap - ch;
    let tab_top = top_y - gap;
    let mut l = Layout { w, h, cw, ch, gap, top_y, tab_top, status_h, down_off: (ch / 9).max(4), up_off: [ch / 4; 7] };
    // Long tableau piles close up so that they stay above the status line.
    let g = &app().game;
    for col in 0..7 {
        let p = &g.piles[TABLEAU + col];
        let downs = p.iter().filter(|c| !c.up).count() as i32;
        let ups = p.iter().filter(|c| c.up).count() as i32;
        let room = tab_top - ch - status_h - gap / 2 - downs * l.down_off;
        if ups > 1 && (ups - 1) * l.up_off[col] > room {
            l.up_off[col] = (room / (ups - 1)).max(6);
        }
    }
    l
}

fn pile_x(l: &Layout, p: usize) -> i32 {
    let col = match p {
        TALON => 0,
        WASTE => 1,
        _ if game::is_foundation(p) => 3 + (p - FOUNDATION) as i32,
        _ => (p - TABLEAU) as i32,
    };
    l.gap + col * (l.cw + l.gap)
}

/// The number of waste cards shown fanned.
fn waste_shown() -> usize {
    let g = &app().game;
    let n = g.piles[WASTE].len();
    if g.draw >= 3 { n.min(3) } else { n.min(1) }
}

/// Where card `i` of pile `p` is (its bottom left).
fn card_pos(l: &Layout, p: usize, i: usize) -> (i32, i32) {
    let g = &app().game;
    let x = pile_x(l, p);
    if game::is_tableau(p) {
        let mut top = l.tab_top;
        for c in &g.piles[p][..i] {
            top -= if c.up { l.up_off[p - TABLEAU] } else { l.down_off };
        }
        return (x, top - l.ch);
    }
    if p == WASTE {
        let n = g.piles[WASTE].len();
        let shown = waste_shown();
        if i + shown >= n {
            let k = i + shown - n;
            return (x + k as i32 * (l.cw / 5), l.top_y);
        }
    }
    (x, l.top_y)
}

fn card_rect(l: &Layout, p: usize, i: usize) -> RectL {
    let (x, y) = card_pos(l, p, i);
    RectL::new(x, y, x + l.cw, y + l.ch)
}

/// The area a drop on pile `p` may land in.
fn drop_rect(l: &Layout, p: usize) -> RectL {
    let n = app().game.piles[p].len();
    let mut r = card_rect(l, p, 0);
    if n > 1 {
        let last = card_rect(l, p, n - 1);
        r.b = r.b.min(last.b);
        r.r = r.r.max(last.r);
    }
    if game::is_tableau(p) {
        r.b = r.b.min(l.status_h);
    }
    r
}

/// The card (pile, index) under a point, or an empty pile's place
/// (pile, None).
fn hit(x: i32, y: i32) -> Option<(usize, Option<usize>)> {
    let a = app();
    let l = a.lay;
    for p in 0..PILES {
        let n = a.game.piles[p].len();
        for i in (0..n).rev() {
            if card_rect(&l, p, i).contains(x, y) {
                return Some((p, Some(i)));
            }
        }
        if n == 0 && card_rect(&l, p, 0).contains(x, y) {
            return Some((p, None));
        }
    }
    None
}

// ---- Drawing ----

fn redraw() {
    let a = app();
    if a.client != 0 {
        invalidate(a.client);
    }
}

fn ensure_pictures() {
    let a = app();
    let r = window_rect(a.client);
    let (w, h) = (r.width().max(1), r.height().max(1));
    let l = layout(w, h);
    let size_changed = a.lay.cw != l.cw;
    a.lay = l;
    if a.atlas.is_none() || size_changed {
        a.atlas = None;
        a.atlas = Atlas::new(a.hab, l.cw, l.ch, a.settings.board());
    }
    if a.back.as_ref().is_none_or(|b| b.cx != w || b.cy != h) {
        a.back = None;
        a.back = MemPs::new(a.hab, w, h);
    }
}

fn picture_of(c: game::Card) -> usize {
    if c.up { c.id as usize } else { 52 + app().settings.card_back as usize }
}

fn render(hps: u32) {
    let a = app();
    let l = a.lay;
    let board = a.settings.board();
    gpi::fill(hps, RectL::new(0, 0, l.w, l.h), board);
    let Some(at) = &a.atlas else { return };
    let g = &a.game;
    for p in 0..PILES {
        let (x, y) = card_pos(&l, p, 0);
        let mut n = g.piles[p].len();
        if let Some(d) = &a.drag {
            if d.from == p {
                n -= d.n;
            }
        }
        if n == 0 {
            let mark: &[u8] = match p {
                TALON if g.can_turn() || g.piles[WASTE].is_empty() => b"O",
                TALON => b"X",
                _ if game::is_foundation(p) => b"A",
                _ if game::is_tableau(p) => b"K",
                _ => b"",
            };
            cards::draw_place(hps, x, y, l.cw, l.ch, board, mark);
            continue;
        }
        let first = match p {
            _ if game::is_tableau(p) => 0,
            WASTE => {
                // The shown cards, or the top under a dragged one.
                let shown = waste_shown();
                g.piles[p].len() - shown
            }
            _ => n - 1,
        };
        for i in first..n {
            let (cx, cy) = card_pos(&l, p, i);
            at.blit(hps, picture_of(g.piles[p][i]), cx, cy);
        }
        if p == TALON {
            // A thicker talon for more cards.
            let edge = (n as i32 / 8).min(3);
            if edge > 0 {
                gpi::color(hps, 0x303030);
                gpi::line(hps, x + 2, y - 1, x + l.cw - 3, y - 1);
            }
        }
    }
    // The selected cards (clicked, waiting for their place).
    if let Some((p, k)) = a.sel {
        let n = g.piles[p].len();
        if k > 0 && k <= n {
            let mut r = card_rect(&l, p, n - k);
            let last = card_rect(&l, p, n - 1);
            r.b = last.b;
            gpi::frame(hps, RectL::new(r.l - 2, r.b - 2, r.r + 2, r.t + 2), 0xffe000);
            gpi::frame(hps, RectL::new(r.l - 3, r.b - 3, r.r + 3, r.t + 3), 0xffe000);
        }
    }
    if a.show_cursor {
        let r = drop_rect(&l, a.cursor);
        gpi::frame(hps, RectL::new(r.l - 4, r.b - 4, r.r + 4, r.t + 4), 0xffffff);
    }
    // The dragged cards.
    if let Some(d) = &a.drag {
        let n = g.piles[d.from].len();
        for (k, c) in g.piles[d.from][n - d.n..].iter().enumerate() {
            let off = if game::is_tableau(d.from) { l.up_off[d.from - TABLEAU].min(l.ch / 4) } else { l.ch / 4 };
            at.blit(hps, picture_of(*c), d.x - d.dx, d.y - d.dy - k as i32 * off);
        }
    }
    status(hps);
}

/// The score, the time and the turns left on the bottom line.
fn status(hps: u32) {
    let a = app();
    let l = a.lay;
    let col = a.settings.text();
    gpi::select_font(hps, 2, b"Helvetica Bold", 14);
    let y = 5;
    if a.settings.score {
        let mut t = b"Score: ".to_vec();
        t.extend_from_slice(&num(a.game.score as i64));
        gpi::text_at(hps, l.gap, y, &t, col);
    }
    let g = &a.game;
    if g.turns_allowed < game::MANY {
        let left = g.turns_allowed.saturating_sub(g.turns);
        let t = alloc::format!("Turns left: {left}").into_bytes();
        let w = gpi::text_width(hps, &t);
        gpi::text_at(hps, (l.w - w) / 2, y, &t, col);
    }
    if a.cheat {
        let t: &[u8] = b"Cheat mode";
        let w = gpi::text_width(hps, t);
        gpi::text_at(hps, l.w * 3 / 4 - w / 2, y, t, 0xffa0a0);
    }
    if a.settings.timer {
        let t = alloc::format!("Time: {}:{:02}", a.secs / 60, a.secs % 60).into_bytes();
        let w = gpi::text_width(hps, &t);
        gpi::text_at(hps, l.w - l.gap - w, y, &t, col);
    }
}

fn paint(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, 0, &mut upd) };
    ensure_pictures();
    match &a.back {
        Some(b) => {
            render(b.hps);
            b.blit_to(hps, 0, 0);
        }
        None => {
            gpi::rgb_mode(hps);
            render(hps);
        }
    }
    unsafe { WinEndPaint(hps) };
}

// ---- Playing ----

/// Make a move; the clock starts with the first, a won game is told.
fn play(m: Move) -> bool {
    let a = app();
    if !a.game.play(m) {
        return false;
    }
    if !a.running && a.secs == 0 {
        a.running = true;
    }
    after_move();
    true
}

fn after_move() {
    let a = app();
    a.sel = None;
    update_menu();
    redraw();
    if a.game.won() && a.running {
        a.running = false;
        a.auto = Auto::Off;
        unsafe { WinStopTimer(a.hab, a.client, TID_AUTO) };
        post(a.client, WM_WON, 0, 0);
    }
}

fn new_deal() {
    let a = app();
    stop_auto();
    apply_rules();
    let deck = game::shuffled(a.rng.as_mut().unwrap());
    a.game.deal(&deck);
    a.secs = 0;
    a.running = false;
    a.sel = None;
    a.drag = None;
    a.cheat = false;
    a.cheat_turns = 0;
    a.lay = Layout::default();
    trace(alloc::format!("new deal, score {}", a.game.score).as_bytes());
    update_menu();
    redraw();
}

/// A click on the talon: turn up cards, or turn the waste pile over.
fn talon_click() {
    let a = app();
    if !a.game.piles[TALON].is_empty() {
        play(Move::Draw { n: 0 });
    } else if a.game.can_turn() {
        play(Move::Turn { n: 0, paid: true });
    } else if !a.game.piles[WASTE].is_empty() {
        unsafe { WinAlarm(HWND_DESKTOP, 0) };
    }
}

/// Move `n` cards from `from` to `to` (in cheat mode also against the
/// rules); a beep when it cannot be done.
fn try_move(from: usize, n: usize, to: usize) -> bool {
    let a = app();
    if from == to {
        return false;
    }
    let legal = a.game.legal(from, to, n);
    let ok = (legal || a.cheat) && play(Move::Transfer { from: from as u8, to: to as u8, n: n as u8, flipped: false, cheat: !legal });
    if !ok {
        unsafe { WinAlarm(HWND_DESKTOP, 0) };
    }
    ok
}

/// The pile a drop at (x, y) goes to: the one whose cards or place the
/// pointer is over, else the one the dropped card overlaps most.
fn drop_target(x: i32, y: i32, card: RectL) -> Option<usize> {
    let l = app().lay;
    let targets = (FOUNDATION..PILES).chain(core::iter::once(WASTE));
    let mut best = None;
    let mut best_area = 0;
    for p in targets {
        let r = drop_rect(&l, p);
        if r.contains(x, y) {
            return Some(p);
        }
        let w = (r.r.min(card.r) - r.l.max(card.l)).max(0);
        let h = (r.t.min(card.t) - r.b.max(card.b)).max(0);
        if w * h > best_area {
            best_area = w * h;
            best = Some(p);
        }
    }
    best
}

fn button1_down(x: i32, y: i32) {
    let a = app();
    stop_auto();
    a.show_cursor = false;
    let Some((p, i)) = hit(x, y) else {
        if a.sel.take().is_some() {
            redraw();
        }
        return;
    };
    if p == TALON {
        a.sel = None;
        talon_click();
        return;
    }
    let n = a.game.piles[p].len();
    // A face-down top card of the tableau turns up when clicked.
    if let Some(i) = i {
        if game::is_tableau(p) && i + 1 == n && !a.game.piles[p][i].up {
            play(Move::Flip { pile: p as u8 });
            return;
        }
    }
    // Clicking the place for selected cards.
    if let Some((from, k)) = a.sel {
        a.sel = None;
        if from != p {
            try_move(from, k, p);
        }
        redraw();
        return;
    }
    let Some(i) = i else { return };
    let movable = a.game.movable(p);
    if i + movable < n || !a.game.piles[p][i].up {
        return;
    }
    let k = n - i;
    if a.settings.drag {
        let (cx, cy) = card_pos(&a.lay, p, i);
        a.drag = Some(Drag { from: p, n: k, dx: x - cx, dy: y - cy, x, y });
        unsafe { WinSetCapture(HWND_DESKTOP, a.client) };
    } else {
        a.sel = Some((p, k));
    }
    redraw();
}

fn button1_up(x: i32, y: i32) {
    let a = app();
    let Some(d) = a.drag.take() else { return };
    unsafe { WinSetCapture(HWND_DESKTOP, 0) };
    let l = a.lay;
    let card = RectL::new(x - d.dx, y - d.dy, x - d.dx + l.cw, y - d.dy + l.ch);
    match drop_target(x, y, card) {
        Some(to) if to != d.from => {
            try_move(d.from, d.n, to);
        }
        _ => {}
    }
    redraw();
}

fn double_click(x: i32, y: i32) {
    let a = app();
    a.sel = None;
    // A quick second click on the talon turns up more cards.
    if let Some((TALON, _)) = hit(x, y) {
        talon_click();
        return;
    }
    let Some((p, Some(i))) = hit(x, y) else { return };
    if i + 1 != a.game.piles[p].len() {
        return;
    }
    if let Some(f) = a.game.foundation_for(p) {
        try_move(p, 1, f);
    }
}

/// The keyboard: the cursor keys pick a pile, Space or Enter acts on it
/// (turns up talon cards, picks cards up, puts them down), Esc drops the
/// choice.
fn key(vk: u16) -> bool {
    let a = app();
    const ORDER: [usize; 13] = [TALON, WASTE, FOUNDATION, FOUNDATION + 1, FOUNDATION + 2, FOUNDATION + 3, TABLEAU, TABLEAU + 1, TABLEAU + 2, TABLEAU + 3, TABLEAU + 4, TABLEAU + 5, TABLEAU + 6];
    let pos = ORDER.iter().position(|&p| p == a.cursor).unwrap_or(6);
    match vk {
        VK_LEFT => a.cursor = ORDER[(pos + 12) % 13],
        VK_RIGHT => a.cursor = ORDER[(pos + 1) % 13],
        VK_UP | VK_DOWN => {
            // Between the top row and the tableau, column by column.
            let col = (pile_x(&a.lay, a.cursor) - a.lay.gap) / (a.lay.cw + a.lay.gap).max(1);
            a.cursor = if game::is_tableau(a.cursor) {
                match col {
                    0 => TALON,
                    1 | 2 => WASTE,
                    c => FOUNDATION + (c as usize - 3),
                }
            } else {
                TABLEAU + col.clamp(0, 6) as usize
            };
        }
        VK_SPACE | VK_ENTER | VK_NEWLINE => {
            stop_auto();
            let p = a.cursor;
            if p == TALON {
                talon_click();
            } else if let Some((from, k)) = a.sel {
                a.sel = None;
                if from != p {
                    try_move(from, k, p);
                }
            } else if let Some(top) = a.game.top(p) {
                if game::is_tableau(p) && !top.up {
                    play(Move::Flip { pile: p as u8 });
                } else {
                    let k = a.game.movable(p);
                    if k > 0 {
                        a.sel = Some((p, k));
                    }
                }
            }
        }
        VK_ESC => a.sel = None,
        _ => return false,
    }
    a.show_cursor = true;
    redraw();
    true
}

// ---- Automatic moves ----

fn stop_auto() {
    let a = app();
    if a.auto != Auto::Off {
        a.auto = Auto::Off;
        unsafe { WinStopTimer(a.hab, a.client, TID_AUTO) };
        update_menu();
    }
}

fn start_auto(mode: Auto) {
    let a = app();
    a.sel = None;
    a.drag = None;
    a.auto = mode;
    if mode == Auto::Finish && !a.settings.animation {
        // All at once.
        while auto_step() {}
        return;
    }
    unsafe { WinStartTimer(a.hab, a.client, TID_AUTO, a.settings.interval()) };
    update_menu();
}

/// One automatic move; false when there is none left.
fn auto_step() -> bool {
    let a = app();
    let m = match a.auto {
        Auto::Off => None,
        Auto::Finish => a.game.finish_step(),
        Auto::Play { idle } => a.game.auto_step(idle),
    };
    match m {
        Some(m) if play(m) => {
            if let Auto::Play { idle } = a.auto {
                a.auto = Auto::Play { idle: if Game::progress(&m) { 0 } else { idle + 1 } };
            }
            true
        }
        _ => {
            stop_auto();
            false
        }
    }
}

fn cheat_remark(n: u32) -> &'static [u8] {
    const R: [&[u8]; 8] = [
        b"Cheat mode is on: cards may go anywhere. Choose Cheat again to play by the rules.",
        b"Cheating again? The cards will not tell anyone.",
        b"Three times now. The cards are starting to talk.",
        b"You do know this is a game of patience?",
        b"Perhaps the rules are just suggestions after all.",
        b"At this rate the cards will deal themselves.",
        b"Some say a game won by cheating is still a game won.",
        b"The kings and queens have stopped counting.",
    ];
    R[(n.saturating_sub(1) as usize).min(R.len() - 1)]
}

fn won() {
    let a = app();
    trace(alloc::format!("won, score {}, {} cheat(s), {} s", a.game.score, a.game.cheats, a.secs).as_bytes());
    let text = match a.game.cheats {
        0 => alloc::format!("All 52 cards are home. Well played!\n\nTime: {}:{:02}", a.secs / 60, a.secs % 60),
        1 => alloc::string::String::from("All 52 cards are home... with one little cheat."),
        n => alloc::format!("All 52 cards are home... with {n} cheats."),
    };
    message(a.frame, text.as_bytes(), b"Congratulations", MB_OK | MB_INFORMATION);
}

fn close() {
    let a = app();
    stop_auto();
    if a.settings != a.saved {
        match message(a.frame, b"You have changed some settings. Do you want to save them before you leave?", TITLE, MB_YESNOCANCEL | MB_ICONQUESTION) {
            MBID_CANCEL => return,
            MBID_YES => {
                settings::save(&a.settings);
                a.saved = a.settings;
            }
            _ => {}
        }
    }
    post(a.client, WM_QUIT, 0, 0);
}

fn command(id: u16) {
    let a = app();
    match id {
        IDM_NEWDEAL => new_deal(),
        IDM_AUTOPLAY => {
            if matches!(a.auto, Auto::Play { .. }) {
                stop_auto();
            } else {
                start_auto(Auto::Play { idle: 0 });
            }
        }
        IDM_TAKEBACK => {
            stop_auto();
            if a.game.take_back().is_some() {
                after_move();
            }
        }
        IDM_REPLAY => {
            stop_auto();
            if a.game.replay().is_some() {
                after_move();
            }
        }
        IDM_CHEAT => {
            a.cheat = !a.cheat;
            if a.cheat {
                a.cheat_turns += 1;
                message(a.frame, cheat_remark(a.cheat_turns), TITLE, MB_OK | MB_INFORMATION);
            }
            update_menu();
            redraw();
        }
        IDM_AUTOFINISH => {
            stop_auto();
            start_auto(Auto::Finish);
        }
        IDM_TIMER => {
            let mut s = a.settings;
            s.timer = !s.timer;
            apply_settings(s);
        }
        IDM_SCOREON | IDM_SCOREOFF => {
            let mut s = a.settings;
            s.score = id == IDM_SCOREON;
            apply_settings(s);
        }
        IDM_SCORERESET => {
            a.game.score = game::START_SCORE;
            redraw();
        }
        1302..=1305 => {
            let mut s = a.settings;
            s.color_set = id - 1302;
            apply_settings(s);
        }
        IDM_SETTINGS => settings::notebook(),
        IDM_SAVE => {
            settings::save(&a.settings);
            a.saved = a.settings;
        }
        IDM_HELPINDEX => help_request(HM_HELP_INDEX, 0, 0),
        IDM_GENERALHELP => help_request(HM_EXT_HELP, 0, 0),
        IDM_USINGHELP => help_request(HM_DISPLAY_HELP, 0, 0),
        IDM_KEYSHELP => help_request(HM_KEYS_HELP, 0, 0),
        IDM_PRODUCT => product_info(a.frame, DLG_PRODUCT, b"Klondike Solitaire - Product Information", b"Klondike Solitaire", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Cards drawn by rs2."]),
        _ => {}
    }
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_SIZE => {
            a.lay = Layout::default();
            redraw();
            0
        }
        WM_ERASEBACKGROUND => 0,
        WM_BUTTON1DOWN => {
            unsafe { WinSetFocus(HWND_DESKTOP, hwnd) };
            let (x, y) = mouse_xy(mp1);
            button1_down(x, y);
            1
        }
        WM_MOUSEMOVE => {
            if let Some(d) = a.drag.as_mut() {
                let (x, y) = mouse_xy(mp1);
                d.x = x;
                d.y = y;
                redraw();
                unsafe { WinUpdateWindow(hwnd) };
            }
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        WM_BUTTON1UP => {
            let (x, y) = mouse_xy(mp1);
            button1_up(x, y);
            1
        }
        WM_BUTTON1DBLCLK => {
            let (x, y) = mouse_xy(mp1);
            double_click(x, y);
            1
        }
        WM_CHAR => {
            let fl = lo(mp1);
            if fl & KC_KEYUP == 0 && fl & KC_VIRTUALKEY != 0 && fl & (KC_CTRL | KC_ALT) == 0 && key(hi(mp2) as u16) {
                return 1;
            }
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        WM_TIMER => {
            match lo(mp1) {
                TID_CLOCK => {
                    if a.running && !a.game.won() {
                        a.secs += 1;
                        if a.settings.timer {
                            redraw();
                        }
                    }
                }
                TID_AUTO => {
                    auto_step();
                }
                _ => {}
            }
            0
        }
        WM_WON => {
            won();
            0
        }
        WM_COMMAND => {
            command(lo(mp1) as u16);
            0
        }
        WM_CLOSE => {
            close();
            0
        }
        WM_SAVEAPPLICATION => {
            if a.settings != a.saved {
                settings::save(&a.settings);
                a.saved = a.settings;
            }
            0
        }
        HM_QUERY_KEYS_HELP => PANEL_KEYS as u32,
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}
