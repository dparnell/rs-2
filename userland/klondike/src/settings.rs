//! The settings: kept, as OS/2's Klondike keeps them, in KLONDIKE.INI
//! (application PATIENCE, values as decimal text: ColorSet, Score,
//! Timer, DragCards, Variation, CardBack, AutoFlip, Animation, Speed,
//! OpeningVideo), here beside the program; and the Settings notebook
//! (Options and Card backs pages; they change the game at once, Undo
//! takes back the page's changes, Default sets OS/2's defaults).

use alloc::vec::Vec;

use pmkit::*;

use crate::{app, cards};

unsafe extern "C" {
    #[link_name = "PMSHAPI.102"]
    fn PrfOpenProfile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMSHAPI.103"]
    fn PrfCloseProfile(hini: u32) -> u32;
}

// Menu ids of the settings (OS/2's).
pub const IDM_SPRING: u16 = 1302;
pub const IDM_VAR_FIRST: u16 = 1351;
pub const IDM_BACK_FIRST: u16 = 1451;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// 0..4: Spring, Summer, Autumn, Winter.
    pub color_set: u16,
    pub score: bool,
    pub timer: bool,
    pub drag: bool,
    /// 0..4: Draw 3 flip none, Draw 3 flip 3, Draw 3 flip many, Draw 1
    /// flip many.
    pub variation: u16,
    pub card_back: u16,
    pub auto_flip: bool,
    pub animation: bool,
    /// 0 fast, 1 medium, 2 slow.
    pub speed: u16,
    pub opening_video: bool,
}

pub const DEFAULTS: Settings = Settings { color_set: 1, score: true, timer: true, drag: true, variation: 2, card_back: 0, auto_flip: true, animation: true, speed: 1, opening_video: false };

impl Settings {
    /// (cards drawn, turns allowed)
    pub fn rules(&self) -> (u32, u32) {
        match self.variation {
            0 => (3, 0),
            1 => (3, 3),
            2 => (3, crate::game::MANY),
            _ => (1, crate::game::MANY),
        }
    }

    /// The timer's interval for automatic moves (ms).
    pub fn interval(&self) -> u32 {
        match self.speed {
            0 => 60,
            1 => 220,
            _ => 600,
        }
    }

    /// The board's colour for the colour scheme.
    pub fn board(&self) -> i32 {
        match self.color_set {
            0 => 0x4f9a52,
            1 => 0x1f6f3a,
            2 => 0x7a4a24,
            _ => 0x35577a,
        }
    }

    /// The colour of the score and time.
    pub fn text(&self) -> i32 {
        match self.color_set {
            0 => 0xfff8c4,
            1 => 0xffffff,
            2 => 0xffd27a,
            _ => 0xe6f0ff,
        }
    }
}

/// KLONDIKE.INI beside the program.
fn profile_path() -> Vec<u8> {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    let mut buf = [0u8; 260];
    if !pib.is_null() && unsafe { os2::DosQueryModuleName((*pib).hmte, 260, buf.as_mut_ptr()) } == 0 {
        let p = until_nul(&buf);
        if let Some(i) = p.iter().rposition(|&c| c == b'\\') {
            return [&p[..=i], b"KLONDIKE.INI"].concat();
        }
    }
    b"KLONDIKE.INI".to_vec()
}

const APP: &[u8] = b"PATIENCE\0";

fn open() -> u32 {
    let p = z(&profile_path());
    unsafe { PrfOpenProfile(app().hab, p.as_ptr()) }
}

fn get(hini: u32, key: &[u8]) -> Option<i64> {
    let k = z(key);
    let mut buf = [0u8; 64];
    let n = unsafe { PrfQueryProfileString(hini, APP.as_ptr(), k.as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), 64) } as usize;
    parse_num(until_nul(&buf[..n.min(64)]))
}

fn put(hini: u32, key: &[u8], v: i64) {
    let k = z(key);
    let s = z(&num(v));
    unsafe { PrfWriteProfileString(hini, APP.as_ptr(), k.as_ptr(), s.as_ptr()) };
}

pub fn load() -> (Settings, Option<[i32; 4]>) {
    let mut s = DEFAULTS;
    let hini = open();
    if hini == 0 {
        return (s, None);
    }
    let b = |v: Option<i64>, d: bool| v.map_or(d, |v| v != 0);
    if let Some(v) = get(hini, b"ColorSet") {
        s.color_set = (v - IDM_SPRING as i64).clamp(0, 3) as u16;
    }
    s.score = b(get(hini, b"Score"), s.score);
    s.timer = b(get(hini, b"Timer"), s.timer);
    s.drag = b(get(hini, b"DragCards"), s.drag);
    if let Some(v) = get(hini, b"Variation") {
        s.variation = (v - IDM_VAR_FIRST as i64).clamp(0, 3) as u16;
    }
    if let Some(v) = get(hini, b"CardBack") {
        s.card_back = (v - IDM_BACK_FIRST as i64).clamp(0, cards::BACKS as i64 - 1) as u16;
    }
    s.auto_flip = b(get(hini, b"AutoFlip"), s.auto_flip);
    s.animation = b(get(hini, b"Animation"), s.animation);
    if let Some(v) = get(hini, b"Speed") {
        s.speed = v.clamp(0, 2) as u16;
    }
    s.opening_video = b(get(hini, b"OpeningVideo"), s.opening_video);
    let pos = [get(hini, b"WindowX"), get(hini, b"WindowY"), get(hini, b"WindowCX"), get(hini, b"WindowCY")];
    let pos = if pos.iter().all(|v| v.is_some()) { Some(pos.map(|v| v.unwrap_or(0) as i32)) } else { None };
    unsafe { PrfCloseProfile(hini) };
    (s, pos)
}

pub fn save(s: &Settings) {
    let hini = open();
    if hini == 0 {
        return;
    }
    put(hini, b"ColorSet", (IDM_SPRING + s.color_set) as i64);
    put(hini, b"Score", s.score as i64);
    put(hini, b"Timer", s.timer as i64);
    put(hini, b"DragCards", s.drag as i64);
    put(hini, b"Variation", (IDM_VAR_FIRST + s.variation) as i64);
    put(hini, b"CardBack", (IDM_BACK_FIRST + s.card_back) as i64);
    put(hini, b"AutoFlip", s.auto_flip as i64);
    put(hini, b"Animation", s.animation as i64);
    put(hini, b"Speed", s.speed as i64);
    put(hini, b"OpeningVideo", s.opening_video as i64);
    unsafe { PrfCloseProfile(hini) };
}

/// The window's place (kept whenever the program ends).
pub fn save_position(swp: &Swp) {
    let hini = open();
    if hini == 0 {
        return;
    }
    put(hini, b"WindowX", swp.x as i64);
    put(hini, b"WindowY", swp.y as i64);
    put(hini, b"WindowCX", swp.cx as i64);
    put(hini, b"WindowCY", swp.cy as i64);
    unsafe { PrfCloseProfile(hini) };
}

// ---- The Settings notebook ----

const BKM_INSERTPAGE: u32 = 0x0355;
const BKM_SETDIMENSIONS: u32 = 0x035e;
const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
const BKM_SETTABTEXT: u32 = 0x0363;
const BKA_LAST: u32 = 0x0002;
const BKA_MAJOR: u32 = 0x0040;
const BKA_STATUSTEXTON: u32 = 0x0001;
const BKA_AUTOPAGESIZE: u32 = 0x0100;
const BKA_MAJORTAB: u32 = 0x0001;
const BKS_BACKPAGESBR: u32 = 0x0001;
const BKS_MAJORTABRIGHT: u32 = 0x0010;
const BKS_SPIRALBIND: u32 = 0x0400;

pub const DLG_SETTINGS: u16 = 2200;
const ID_NOTEBOOK: u32 = 2201;

// The Options page (OS/2's ids).
pub const PAGE_OPTIONS: u16 = 2210;
const ID_VIDEO: u32 = 2216;
const ID_DRAG: u32 = 2213;
const ID_AUTOFLIP: u32 = 2214;
const ID_ANIMATE: u32 = 2215;
const ID_FAST: u32 = 2233;
const ID_MEDIUM: u32 = 2234;
const ID_SLOW: u32 = 2235;
const ID_UNDO: u32 = 2290;
const ID_DEFAULT: u32 = 2291;
const ID_OPT_HELP: u32 = 2292;

// The Card backs page.
pub const PAGE_BACKS: u16 = 2240;
const ID_BACKS: u32 = 2242;
const ID_BACK_UNDO: u32 = 2245;
const ID_BACK_DEFAULT: u32 = 2246;
const ID_BACK_HELP: u32 = 2248;

/// What the pages started with (their Undo).
static mut UNDO: Settings = DEFAULTS;
/// The notebook's dialog (a page's Close or Esc closes it).
static mut NOTEBOOK: u32 = 0;

fn close_notebook() {
    let d = unsafe { NOTEBOOK };
    if d != 0 {
        post(d, WM_CLOSE, 0, 0);
    }
}

fn page_frame(id: u16) -> Item {
    pmkit::tpl::item(WC_FRAME, b"", 0, (0, 0, 212, 136), id)
}

fn options_page() -> Vec<u8> {
    template(
        page_frame(PAGE_OPTIONS),
        alloc::vec![
            group_box(b"Options", (4, 22, 102, 112)),
            check_box(b"Opening video", (8, 108, 94, 10), ID_VIDEO),
            check_box(b"Drag cards", (8, 96, 94, 10), ID_DRAG),
            check_box(b"Auto-flip cards", (8, 84, 94, 10), ID_AUTOFLIP),
            check_box(b"Animate cards", (8, 72, 94, 10), ID_ANIMATE),
            group_box(b"Animation speed", (8, 26, 94, 42)),
            radio(b"Fast", (12, 50, 80, 10), ID_FAST).group(),
            radio(b"Medium", (12, 40, 80, 10), ID_MEDIUM),
            radio(b"Slow", (12, 30, 80, 10), ID_SLOW),
            group_box(b"Game variations", (108, 72, 100, 62)),
            radio(b"~Draw 3, Flip none", (112, 110, 92, 10), 1351).group(),
            radio(b"D~raw 3, Flip 3", (112, 99, 92, 10), 1352),
            radio(b"Dr~aw 3, Flip many", (112, 88, 92, 10), 1353),
            radio(b"Dra~w 1, Flip many", (112, 77, 92, 10), 1354),
            button(b"~Undo", (4, 2, 44), ID_UNDO, 0),
            button(b"~Default", (52, 2, 44), ID_DEFAULT, 0),
            button(b"Help", (162, 2, 44), ID_OPT_HELP, BS_NOPOINTERFOCUS),
        ],
    )
}

fn backs_page() -> Vec<u8> {
    template(
        page_frame(PAGE_BACKS),
        alloc::vec![
            group_box(b"Card backs", (4, 22, 204, 112)),
            custom(b"KlondikeBacks", (10, 26, 192, 98), ID_BACKS, WS_TABSTOP),
            button(b"~Undo", (4, 2, 44), ID_BACK_UNDO, 0),
            button(b"~Default", (52, 2, 44), ID_BACK_DEFAULT, 0),
            button(b"Help", (162, 2, 44), ID_BACK_HELP, BS_NOPOINTERFOCUS),
        ],
    )
}

fn show_options(h: u32, s: &Settings) {
    set_check(h, ID_VIDEO, s.opening_video);
    set_check(h, ID_DRAG, s.drag);
    set_check(h, ID_AUTOFLIP, s.auto_flip);
    set_check(h, ID_ANIMATE, s.animation);
    for (i, id) in [ID_FAST, ID_MEDIUM, ID_SLOW].into_iter().enumerate() {
        set_check(h, id, s.speed as usize == i);
    }
    for v in 0..4u16 {
        set_check(h, (IDM_VAR_FIRST + v) as u32, s.variation == v);
    }
}

extern "C" fn options_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            show_options(h, &app().settings);
            // rs2 has no opening video.
            enable(h, ID_VIDEO, false);
            0
        }
        WM_CONTROL if hi(mp1) == BN_CLICKED => {
            let id = lo(mp1);
            let mut s = app().settings;
            match id {
                ID_DRAG => s.drag = checked(h, ID_DRAG),
                ID_AUTOFLIP => s.auto_flip = checked(h, ID_AUTOFLIP),
                ID_ANIMATE => s.animation = checked(h, ID_ANIMATE),
                ID_FAST => s.speed = 0,
                ID_MEDIUM => s.speed = 1,
                ID_SLOW => s.speed = 2,
                1351..=1354 => s.variation = (id - 1351) as u16,
                _ => return 0,
            }
            crate::apply_settings(s);
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_UNDO => {
                    let u = unsafe { UNDO };
                    let mut s = app().settings;
                    (s.drag, s.auto_flip, s.animation, s.speed, s.variation, s.opening_video) = (u.drag, u.auto_flip, u.animation, u.speed, u.variation, u.opening_video);
                    crate::apply_settings(s);
                    show_options(h, &s);
                }
                ID_DEFAULT => {
                    let d = DEFAULTS;
                    let mut s = app().settings;
                    (s.drag, s.auto_flip, s.animation, s.speed, s.variation, s.opening_video) = (d.drag, d.auto_flip, d.animation, d.speed, d.variation, d.opening_video);
                    crate::apply_settings(s);
                    show_options(h, &s);
                }
                ID_OPT_HELP => crate::show_help(PAGE_OPTIONS),
                DID_CANCEL => close_notebook(),
                _ => {}
            }
            0
        }
        // Enter and Esc on a page must not end it.
        WM_CLOSE => {
            close_notebook();
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

extern "C" fn backs_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_COMMAND => {
            let mut s = app().settings;
            match lo(mp1) {
                ID_BACK_UNDO => s.card_back = unsafe { UNDO.card_back },
                ID_BACK_DEFAULT => s.card_back = DEFAULTS.card_back,
                DID_CANCEL => {
                    close_notebook();
                    return 0;
                }
                ID_BACK_HELP => {
                    crate::show_help(PAGE_BACKS);
                    return 0;
                }
                _ => return 0,
            }
            crate::apply_settings(s);
            invalidate(item(h, ID_BACKS));
            0
        }
        WM_CLOSE => {
            close_notebook();
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// The nine backs, three by three, the chosen one framed; a click (or
/// the cursor keys) chooses.
extern "C" fn backs_window(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let cell = |r: &RectL, i: usize| {
        let (cw, ch) = (r.width() / 3, r.height() / 3);
        let (col, row) = ((i % 3) as i32, (i / 3) as i32);
        RectL::new(r.l + col * cw, r.t - (row + 1) * ch, r.l + (col + 1) * cw, r.t - row * ch)
    };
    match m {
        WM_PAINT => {
            let mut upd = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut upd) };
            let r = window_rect(h);
            gpi::rgb_mode(hps);
            gpi::fill(hps, r, unsafe { WinQuerySysColor(HWND_DESKTOP, SYSCLR_DIALOGBACKGROUND, 0) } & 0xffffff);
            let a = app();
            for i in 0..cards::BACKS {
                let c = cell(&r, i);
                let ch = c.height() - 8;
                let cw = ch * 5 / 7;
                let x = (c.l + c.r - cw) / 2;
                if let Some(at) = &a.atlas {
                    let (sx, sy) = (i as i32 * at.w, 4 * at.h);
                    let p = [pt(x, c.b + 4), pt(x + cw, c.b + 4 + ch), pt(sx, sy), pt(sx + at.w, sy + at.h)];
                    unsafe { GpiBitBlt(hps, at.ps.hps, 4, p.as_ptr(), ROP_SRCCOPY, BBO_IGNORE) };
                }
                if i == a.settings.card_back as usize {
                    gpi::frame(hps, RectL::new(x - 3, c.b + 1, x + cw + 3, c.b + 7 + ch), 0x000080);
                    gpi::frame(hps, RectL::new(x - 2, c.b + 2, x + cw + 2, c.b + 6 + ch), 0x000080);
                }
            }
            unsafe { WinEndPaint(hps) };
            0
        }
        WM_BUTTON1DOWN => {
            let (x, y) = mouse_xy(mp1);
            let r = window_rect(h);
            if let Some(i) = (0..cards::BACKS).find(|&i| cell(&r, i).contains(x, y)) {
                let mut s = app().settings;
                s.card_back = i as u16;
                crate::apply_settings(s);
                invalidate(h);
            }
            unsafe { WinSetFocus(HWND_DESKTOP, h) };
            1
        }
        WM_CHAR => {
            let fl = lo(mp1);
            if fl & KC_KEYUP == 0 && fl & KC_VIRTUALKEY != 0 {
                let mut s = app().settings;
                let b = s.card_back as i32;
                let nb = match hi(mp2) as u16 {
                    VK_LEFT => b - 1,
                    VK_RIGHT => b + 1,
                    VK_UP => b - 3,
                    VK_DOWN => b + 3,
                    _ => return unsafe { WinDefWindowProc(h, m, mp1, mp2) },
                };
                if (0..cards::BACKS as i32).contains(&nb) {
                    s.card_back = nb as u16;
                    crate::apply_settings(s);
                    invalidate(h);
                }
                return 1;
            }
            unsafe { WinDefWindowProc(h, m, mp1, mp2) }
        }
        _ => unsafe { WinDefWindowProc(h, m, mp1, mp2) },
    }
}

extern "C" fn notebook_dlg(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            unsafe { NOTEBOOK = h };
            let nb = item(h, ID_NOTEBOOK);
            send(nb, BKM_SETDIMENSIONS, mp(90, 24), BKA_MAJORTAB);
            let pages: [(&[u8], &[u8], extern "C" fn(u32, u32, u32, u32) -> u32, Vec<u8>); 2] =
                [(b"~Options\0", b"Options\0", options_proc, options_page()), (b"Card ~backs\0", b"Card backs\0", backs_proc, backs_page())];
            for (tab, status, proc_, tpl) in pages.iter() {
                let id = send(nb, BKM_INSERTPAGE, 0, mp(BKA_MAJOR | BKA_STATUSTEXTON | BKA_AUTOPAGESIZE, BKA_LAST));
                let page = unsafe { WinCreateDlg(nb, nb, *proc_ as *const () as u32, tpl.as_ptr(), 0) };
                send(nb, BKM_SETPAGEWINDOWHWND, id, page);
                send(nb, BKM_SETTABTEXT, id, tab.as_ptr() as u32);
                send(nb, BKM_SETSTATUSLINETEXT, id, status.as_ptr() as u32);
            }
            0
        }
        WM_CLOSE => {
            unsafe { WinDismissDlg(h, DID_OK) };
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Options > Settings: the notebook.
pub fn notebook() {
    let a = app();
    unsafe {
        UNDO = a.settings;
        WinRegisterClass(a.hab, b"KlondikeBacks\0".as_ptr(), backs_window, CS_SIZEREDRAW, 0);
    }
    let f = dialog_frame(b"Klondike Solitaire - Settings", DLG_SETTINGS, 272, 168);
    let nb = pmkit::tpl::item(WC_NOTEBOOK, b"", WS_VISIBLE | WS_TABSTOP | BKS_BACKPAGESBR | BKS_MAJORTABRIGHT | BKS_SPIRALBIND, (0, 0, 272, 168), ID_NOTEBOOK as u16);
    let tpl = template(f, alloc::vec![nb]);
    run_dialog(a.frame, notebook_dlg, &tpl, 0);
    unsafe { NOTEBOOK = 0 };
}
