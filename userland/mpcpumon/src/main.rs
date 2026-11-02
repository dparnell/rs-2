//! MPCPUMON.EXE: rs2's Pulse (the CPU monitor), a re-implementation of
//! OS/2's "OS/2 SMP Monitor".
//!
//! As OS/2's (studied in its binary): one copy at a time (the named shared
//! memory `\SHAREMEM\MPCPUMON`), a frame with a menu bar (Graph Mode: Bar
//! or Line; Status; Options: Background Color, Processor Graph Color,
//! Horizontal and Vertical Labels, Freeze Screen, Grid, Sample Rate;
//! Help), the same command ids and Ctrl accelerators, a pop-up menu on
//! mouse button 2, double-click hiding and showing the title bar and
//! menu, colours dropped from the Color Palette setting a processor's
//! colour (or the background), and the processors' load read through the
//! performance counters (perf.rs) every sample period (150 ms unless set
//! otherwise). The graph is "% Processor Utilization" with 0-100 labels
//! up the left and the processors along the bottom: a line per processor
//! moving right to left, or a bar per processor and one for their
//! average. Status lists the processors online and offline and asks
//! DosSetProcessorStatus to change them; rs2 keeps every processor
//! online, so turning one off reports that its status could not be
//! changed, as OS/2 does when the call fails.
//!
//! OS/2's Pulse keeps no settings; rs2's keeps its window position and
//! options in OS2.INI (application `MPCPUMON`).
//!
//! `MPCPUMON /SELFTEST` checks the counters and the load arithmetic and
//! prints PASS/FAIL lines and "mpcpumon selftest done: N failure(s)".

#![no_std]
#![no_main]

extern crate alloc;

mod dialogs;
mod perf;
mod selftest;

use alloc::vec::Vec;

use pmkit::gpi;
use pmkit::help::{Help, HelpWindow};
use pmkit::*;

use perf::{Perf, HISTORY};

os2::entry!(main);

// Commands (OS/2's ids).
pub const IDM_BAR: u16 = 1000;
pub const IDM_LINE: u16 = 1010;
pub const IDM_STATUS: u16 = 1030;
pub const IDM_OPTIONS: u16 = 1040;
pub const IDM_HELP: u16 = 1050;
pub const IDM_HELPINDEX: u16 = 1051;
pub const IDM_GENERALHELP: u16 = 1052;
pub const IDM_USINGHELP: u16 = 1053;
pub const IDM_KEYSHELP: u16 = 1054;
pub const IDM_PRODUCT: u16 = 1055;
pub const IDM_GRAPHMODE: u16 = 1061;
pub const IDM_HLABELS: u16 = 1062;
pub const IDM_VLABELS: u16 = 1064;
pub const IDM_FREEZE: u16 = 1066;
pub const IDM_GRID: u16 = 1068;
pub const IDM_SAMPLERATE: u16 = 1070;
pub const IDM_BACKGROUND: u16 = 1100;
pub const IDM_CPUCOLOR: u16 = 1200;

// Dialogs.
pub const DLG_STATUS: u16 = 301;
pub const DLG_RATE: u16 = 303;
pub const DLG_PRODUCT: u16 = 305;
pub const DLG_COLOR: u16 = 307;

// Help panels (OS/2's numbers).
pub const PANEL_GENERAL: u16 = 9000;
pub const PANEL_KEYS: u16 = 6000;
pub const PANEL_COLORDLG: u16 = 4310;
pub const PANEL_PRODUCT: u16 = 1055;

// Option flags (OS/2's bits).
pub const F_VLABELS: u32 = 0x01;
pub const F_HLABELS: u32 = 0x02;
pub const F_TITLE: u32 = 0x04;
pub const F_GRID: u32 = 0x08;
pub const F_FREEZE: u32 = 0x10;
pub const F_BAR: u32 = 0x100;
pub const F_LINE: u32 = 0x200;

const FRAME_ID: u32 = 300;
const TIMER_ID: u32 = 1000;
pub const TITLE: &[u8] = b"CPU Monitor";
const HELP_FILE: &[u8] = b"MPCPUMON.HLP";
const INI_APP: &[u8] = b"MPCPUMON";

/// The colours of the menus, in their order: names and RGB values.
pub const COLORS: [(&[u8], i32); 16] = [
    (b"Pale Gray", 0xcccccc),
    (b"Brown", 0x808000),
    (b"Blue", 0x0000ff),
    (b"Red", 0xff0000),
    (b"Green", 0x00ff00),
    (b"Pink", 0xff00ff),
    (b"Cyan", 0x00ffff),
    (b"Black", 0x000000),
    (b"Dark Gray", 0x808080),
    (b"Yellow", 0xffff00),
    (b"Dark Blue", 0x000080),
    (b"Dark Red", 0x800000),
    (b"Dark Green", 0x008000),
    (b"Dark Pink", 0x800080),
    (b"Dark Cyan", 0x008080),
    (b"White", 0xffffff),
];

/// What is kept in OS2.INI.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Settings {
    pub flags: u32,
    pub rate: u32,
    pub background: u8,
    /// The processors' colours (COLORS indexes); the average's after
    /// them.
    pub colors: [u8; 17],
}

impl Settings {
    pub const fn default() -> Settings {
        // Each processor starts with the next colour of the list, as on
        // OS/2: CPU 1 brown, CPU 2 blue, CPU 3 red...
        let mut colors = [0u8; 17];
        let mut i = 0;
        while i < 17 {
            colors[i] = (i % 15) as u8 + 1;
            i += 1;
        }
        Settings { flags: F_VLABELS | F_HLABELS | F_TITLE | F_LINE, rate: 150, background: 0, colors }
    }
}

pub struct App {
    pub hab: u32,
    pub frame: u32,
    pub client: u32,
    pub menu: u32,
    pub help: Option<&'static mut Help>,
    pub perf: Option<Perf>,
    pub set: Settings,
    /// The history shown while the screen is frozen.
    pub frozen: Option<Vec<Vec<u8>>>,
    /// The frame controls are hidden (double-click).
    pub bare: bool,
    pub back: Option<gpi::MemPs>,
}

static mut APP: App = App { hab: 0, frame: 0, client: 0, menu: 0, help: None, perf: None, set: Settings::default(), frozen: None, bare: false, back: None };

pub fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

pub fn trace(s: &[u8]) {
    pmkit::trace("mpcpumon", s);
}

static mut FRAME_PROC: u32 = 0;

fn main(s: &os2::Startup) -> u32 {
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        return 1;
    }
    if pmkit::arg(s).eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    // One copy at a time.
    let mut shared = core::ptr::null_mut::<u8>();
    if unsafe { os2::DosAllocSharedMem(&mut shared, b"\\SHAREMEM\\MPCPUMON\0".as_ptr(), 4096, 0x13) } != 0 {
        message(0, b"The CPU Monitor is already running. Only one copy can run at a time.", TITLE, MB_OK | MB_WARNING);
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return 1;
    }
    let mut st = Settings::default();
    if ini::read_struct(INI_APP, b"SETTINGS", &mut st) {
        st.flags &= !F_FREEZE;
        if st.flags & (F_BAR | F_LINE) == 0 {
            st.flags |= F_LINE;
        }
        st.rate = st.rate.clamp(50, 30000);
        a.set = st;
    }
    a.perf = Perf::open();
    if a.perf.is_none() {
        message(0, b"The processors' performance counters cannot be read.", TITLE, MB_OK | MB_ERROR);
    }
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(a.hab, b"SMP Monitor", client_proc, CS_SIZEREDRAW, 0, fcf, TITLE, FRAME_ID);
    if frame == 0 {
        return 1;
    }
    a.frame = frame;
    a.client = client;
    set_font(client, b"8.Helv");
    a.menu = set_menu(frame, &menu_bar());
    let mut acc = Vec::new();
    for (c, cmd) in [(b'B', IDM_BAR), (b'L', IDM_LINE), (b'S', IDM_STATUS), (b'R', IDM_HLABELS), (b'V', IDM_VLABELS), (b'F', IDM_FREEZE), (b'G', IDM_GRID), (b'A', IDM_SAMPLERATE)] {
        acc.push((AF_CHAR | AF_CONTROL, c as u16, cmd));
        acc.push((AF_CHAR | AF_CONTROL, c.to_ascii_lowercase() as u16, cmd));
    }
    set_accels(a.hab, frame, &acc);
    update_menu();
    let old = unsafe { WinSubclassWindow(frame, frame_proc) };
    unsafe { FRAME_PROC = old };
    a.help = create_help();
    unsafe { WinStartTimer(a.hab, client, TIMER_ID, a.set.rate) };
    // OS/2's starts wide and low in the middle of the screen.
    let (sw, sh) = screen();
    show_at_saved(a.hab, frame, INI_APP, b"WINDOWPOS", Some((sw * 3 / 4, (sh * 2 / 5).max(240))));
    message_loop(a.hab);
    if a.bare {
        show_controls(true);
    }
    save_position(frame, INI_APP, b"WINDOWPOS");
    let mut keep = a.set;
    keep.flags &= !F_FREEZE;
    ini::write_struct(INI_APP, b"SETTINGS", &keep);
    unsafe {
        WinStopTimer(a.hab, client, TIMER_ID);
        if let Some(h) = a.help.as_mut() {
            h.destroy(frame);
        }
        a.back = None;
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
        os2::DosFreeMem(shared);
    }
    if a.perf.is_some() {
        Perf::close();
    }
    0
}

fn cpus() -> usize {
    app().perf.as_ref().map_or(0, |p| p.count())
}

fn menu_bar() -> Vec<Menu<'static>> {
    let n = cpus().clamp(1, 16);
    let bg: Vec<Menu> = COLORS.iter().enumerate().map(|(i, c)| Menu::Item(IDM_BACKGROUND + 1 + i as u16, c.0)).collect();
    let cpu: Vec<Menu> = CPU_NAMES[..n].iter().enumerate().map(|(i, t)| Menu::Item(IDM_CPUCOLOR + 1 + i as u16, t)).collect();
    alloc::vec![
        Menu::Sub(IDM_GRAPHMODE, b"~Graph Mode", alloc::vec![Menu::Item(IDM_BAR, b"~Bar\tCtrl+B"), Menu::Item(IDM_LINE, b"~Line\tCtrl+L")]),
        Menu::Item(IDM_STATUS, b"~Status"),
        Menu::Sub(
            IDM_OPTIONS,
            b"~Options",
            alloc::vec![
                Menu::Sub(IDM_BACKGROUND, b"~Background Color", bg),
                Menu::Sub(IDM_CPUCOLOR, b"~Processor Graph Color", cpu),
                Menu::Sep,
                Menu::Item(IDM_HLABELS, b"Ho~rizontal Labels\tCtrl+R"),
                Menu::Item(IDM_VLABELS, b"~Vertical Labels\tCtrl+V"),
                Menu::Sep,
                Menu::Item(IDM_FREEZE, b"~Freeze Screen\tCtrl+F"),
                Menu::Item(IDM_GRID, b"~Grid\tCtrl+G"),
                Menu::Sep,
                Menu::Item(IDM_SAMPLERATE, b"S~ample Rate...\tCtrl+A"),
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

const CPU_NAMES: [&[u8]; 16] = [
    b"CPU ~1", b"CPU ~2", b"CPU ~3", b"CPU ~4", b"CPU ~5", b"CPU ~6", b"CPU ~7", b"CPU ~8", b"CPU ~9", b"CPU 1~0", b"CPU 11", b"CPU 12", b"CPU 13", b"CPU 14", b"CPU 15",
    b"CPU 16",
];

/// The pop-up menu's items (with their checks).
fn popup_items() -> Vec<Menu<'static>> {
    let f = app().set.flags;
    alloc::vec![
        Menu::Sub(IDM_GRAPHMODE, b"~Processor Time", alloc::vec![Menu::Check(IDM_BAR, b"~Bar\tCtrl+B", f & F_BAR != 0), Menu::Check(IDM_LINE, b"~Line\tCtrl+L", f & F_LINE != 0)]),
        Menu::Item(IDM_STATUS, b"Processor ~Status...\tCtrl+S"),
        Menu::Sep,
        Menu::Check(IDM_FREEZE, b"~Freeze Screen\tCtrl+F", f & F_FREEZE != 0),
        Menu::Check(IDM_GRID, b"~Grid\tCtrl+G", f & F_GRID != 0),
        Menu::Sep,
        Menu::Item(IDM_SAMPLERATE, b"S~ample Rate...\tCtrl+A"),
    ]
}

/// The menu bar's check marks for the options.
fn update_menu() {
    let a = app();
    let f = a.set.flags;
    menu_check(a.menu, IDM_BAR, f & F_BAR != 0);
    menu_check(a.menu, IDM_LINE, f & F_LINE != 0);
    menu_check(a.menu, IDM_HLABELS, f & F_HLABELS != 0);
    menu_check(a.menu, IDM_VLABELS, f & F_VLABELS != 0);
    menu_check(a.menu, IDM_FREEZE, f & F_FREEZE != 0);
    menu_check(a.menu, IDM_GRID, f & F_GRID != 0);
    for i in 0..16u16 {
        menu_check(a.menu, IDM_BACKGROUND + 1 + i, a.set.background as u16 == i);
    }
    menu_enable(a.menu, IDM_STATUS, a.perf.is_some());
}

fn create_help() -> Option<&'static mut Help> {
    let a = app();
    // OS/2's help table: menu items to panels; every background colour
    // shows 4110, every processor colour 4210.
    let mut items: Vec<(u16, u16)> = alloc::vec![
        (IDM_GRAPHMODE, 1061),
        (IDM_BAR, 1000),
        (IDM_LINE, 1010),
        (IDM_STATUS, 1030),
        (IDM_OPTIONS, 1040),
        (IDM_BACKGROUND, 1100),
        (IDM_CPUCOLOR, 1200),
        (IDM_HLABELS, 1062),
        (IDM_VLABELS, 1064),
        (IDM_FREEZE, 1066),
        (IDM_GRID, 1068),
        (IDM_SAMPLERATE, 1070),
        (IDM_PRODUCT, PANEL_PRODUCT),
    ];
    for i in 1..=16 {
        items.push((IDM_BACKGROUND + i, 4110));
        items.push((IDM_CPUCOLOR + i, 4210));
    }
    let items: &'static [(u16, u16)] = alloc::boxed::Box::leak(items.into_boxed_slice());
    Help::create(
        a.hab,
        a.frame,
        HELP_FILE,
        b"CPU Monitor Help",
        &[
            HelpWindow { id: FRAME_ID as u16, ext: PANEL_GENERAL, items },
            HelpWindow { id: DLG_STATUS, ext: 1030, items: &[(dialogs::ID_CPULIST as u16, 1030), (1, 1030), (2, 1030)] },
            HelpWindow { id: DLG_RATE, ext: 1070, items: &[(dialogs::ID_RATE as u16, 1070), (1, 1070), (2, 1070)] },
            HelpWindow { id: DLG_COLOR, ext: PANEL_COLORDLG, items: &[(dialogs::ID_COLORLIST as u16, PANEL_COLORDLG), (1, PANEL_COLORDLG), (2, PANEL_COLORDLG)] },
            HelpWindow { id: DLG_PRODUCT, ext: PANEL_PRODUCT, items: &[(1, PANEL_PRODUCT)] },
        ],
    )
}

fn help_request(msg: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, 0, 0);
}

/// Hide (or show again) the title bar, system menu, min/max buttons and
/// menu bar, as a double-click does.
fn show_controls(show: bool) {
    let a = app();
    for id in [FID_TITLEBAR, FID_SYSMENU, FID_MINMAX] {
        let h = if show { unsafe { WinWindowFromID(HWND_OBJECT, id) } } else { unsafe { WinWindowFromID(a.frame, id) } };
        if h != 0 {
            unsafe { WinSetParent(h, if show { a.frame } else { HWND_OBJECT }, 0) };
        }
    }
    unsafe { WinSetParent(a.menu, if show { a.frame } else { HWND_OBJECT }, 0) };
    send(a.frame, WM_UPDATEFRAME, FCF_TITLEBAR | FCF_SYSMENU | FCF_MENU | FCF_MINMAX, 0);
    unsafe { WinInvalidateRect(a.frame, core::ptr::null(), 1) };
    a.bare = !show;
}

extern "C" fn frame_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let old: WndProc = unsafe { core::mem::transmute::<u32, WndProc>(FRAME_PROC) };
    if m == WM_ADJUSTWINDOWPOS && mp1 != 0 {
        // Not smaller than the graph needs.
        let swp = unsafe { &mut *(mp1 as *mut Swp) };
        if swp.fl & SWP_SIZE != 0 {
            swp.cx = swp.cx.max(260);
            swp.cy = swp.cy.max(140);
        }
    }
    old(h, m, mp1, mp2)
}

// ---- The graph ----

/// Where things go in the client window.
pub struct Layout {
    pub graph: RectL,
    pub fh: i32,
    pub desc: i32,
    pub label_w: i32,
}

pub fn layout(hps: u32, w: i32, h: i32, flags: u32) -> Layout {
    let (fh, desc) = gpi::font_height(hps);
    let fh = fh.max(8);
    let label_w = gpi::text_width(hps, b"100");
    let left = if flags & F_VLABELS != 0 { label_w + 14 } else { 6 };
    let top = h - if flags & F_TITLE != 0 { fh + 10 } else { 6 };
    let bottom = if flags & F_HLABELS != 0 { fh * 2 + 10 } else { 6 };
    Layout { graph: RectL::new(left, bottom, (w - 8).max(left + 10), top.max(bottom + 10)), fh, desc, label_w }
}

/// A load's height in the graph.
fn y_of(g: &RectL, pct: u8) -> i32 {
    g.b + (g.height() - 1) * pct as i32 / 100
}

/// Bars: (left, right) of processor i's bar (i == n: the average).
pub fn bar_span(g: &RectL, n: usize, i: usize) -> (i32, i32) {
    let slot = g.width() / (n as i32 + 2);
    let q = slot / 4;
    let x = if i < n { g.l + q * 2 + i as i32 * slot } else { g.l + q * 4 + n as i32 * slot };
    (x, x + (slot / 2).max(2))
}

/// The processor (or average, `n`) whose graph is at (x, y), if any.
pub fn hit(g: &RectL, hist: &[Vec<u8>], flags: u32, x: i32, y: i32) -> Option<usize> {
    if !g.contains(x, y) || hist.is_empty() {
        return None;
    }
    let n = hist.len() - 1;
    if flags & F_BAR != 0 {
        (0..=n).find(|&i| {
            let (l, r) = bar_span(g, n, i);
            x >= l && x < r && y <= y_of(g, *hist[i].last().unwrap_or(&0)).max(g.b + 3)
        })
    } else {
        let j = ((x - g.l) * (HISTORY as i32 - 1) / (g.width() - 1).max(1)).clamp(0, HISTORY as i32 - 1) as usize;
        (0..n).filter(|&i| (y_of(g, hist[i][j]) - y).abs() <= 6).min_by_key(|&i| (y_of(g, hist[i][j]) - y).abs())
    }
}

fn color_of(i: usize) -> i32 {
    COLORS[app().set.colors[i.min(16)] as usize % 16].1
}

fn number(n: usize) -> Vec<u8> {
    alloc::format!("{n}").into_bytes()
}

/// Draw everything into a PS (RGB colours) of `w` by `h` pels.
pub fn draw(hps: u32, w: i32, h: i32) {
    let a = app();
    let f = a.set.flags;
    let face = unsafe { WinQuerySysColor(HWND_DESKTOP, SYSCLR_DIALOGBACKGROUND, 0) };
    let text = 0x000000;
    let edge = 0x000080;
    gpi::fill(hps, RectL::new(0, 0, w, h), face);
    let lay = layout(hps, w, h, f);
    let g = lay.graph;
    let empty: Vec<Vec<u8>> = Vec::new();
    let hist: &Vec<Vec<u8>> = match (&a.frozen, &a.perf) {
        (Some(fz), _) => fz,
        (None, Some(p)) => &p.history,
        _ => &empty,
    };
    let n = hist.len().saturating_sub(1);
    // The title, boxed, over the graph.
    if f & F_TITLE != 0 {
        let t: &[u8] = b"% Processor Utilization";
        let tw = gpi::text_width(hps, t);
        let x = (w - tw) / 2;
        let y = h - lay.fh - 4;
        gpi::frame(hps, RectL::new(x - 3, y - 2, x + tw + 3, y + lay.fh + 1), edge);
        gpi::text_at(hps, x, y + lay.desc, t, text);
    }
    // The graph's area, grid and frame.
    gpi::fill(hps, g, COLORS[a.set.background as usize % 16].1);
    if f & F_GRID != 0 {
        let gc = if a.set.background == 8 { 0xcccccc } else { 0x808080 };
        unsafe { GpiSetLineType(hps, LINETYPE_DOT) };
        for k in 1..10 {
            let y = g.b + (g.height() - 1) * k / 10;
            gpi::color(hps, gc);
            gpi::line(hps, g.l, y, g.r - 1, y);
        }
        unsafe { GpiSetLineType(hps, LINETYPE_SOLID) };
    }
    gpi::frame(hps, g, edge);
    // 0..100 up the left, with ticks.
    if f & F_VLABELS != 0 {
        let box_r = RectL::new(2, g.b - lay.fh / 2 - 1, g.l - 2, g.t + lay.fh / 2 + 1);
        gpi::frame(hps, box_r, edge);
        for k in 0..=10 {
            let y = g.b + (g.height() - 1) * k / 10;
            let s = number(k as usize * 10);
            let sw = gpi::text_width(hps, &s);
            gpi::text_at(hps, g.l - 8 - sw, y - lay.fh / 2 + lay.desc + 1, &s, text);
            gpi::color(hps, edge);
            gpi::line(hps, g.l - 5, y, g.l - 1, y);
        }
    }
    if n == 0 {
        if a.perf.is_none() {
            gpi::text_at(hps, g.l + 8, g.t - lay.fh - 4, b"No performance counters.", text);
        }
        return;
    }
    if f & F_BAR != 0 {
        for i in 0..=n {
            let (l, r) = bar_span(&g, n, i);
            let v = *hist[i].last().unwrap_or(&0);
            let top = y_of(&g, v);
            let c = color_of(i);
            gpi::fill(hps, RectL::new(l, g.b + 1, r, top + 1), c);
            gpi::frame(hps, RectL::new(l, g.b + 1, r, top + 1), edge);
            // The value over the bar.
            let s = alloc::format!("{v}%").into_bytes();
            let sw = gpi::text_width(hps, &s);
            if top + lay.fh + 2 < g.t {
                gpi::text_at(hps, (l + r - sw) / 2, top + 3 + lay.desc, &s, text);
            }
        }
    } else {
        let mut pts = alloc::vec![PointL::default(); HISTORY];
        for i in 0..n {
            for (j, p) in pts.iter_mut().enumerate() {
                p.x = g.l + 1 + (g.width() - 3) * j as i32 / (HISTORY as i32 - 1);
                p.y = y_of(&g, hist[i][j]);
            }
            gpi::color(hps, color_of(i));
            unsafe {
                GpiSetLineWidth(hps, 0x20000);
                GpiMove(hps, &pts[0]);
                GpiPolyLine(hps, HISTORY as i32 - 1, pts[1..].as_ptr());
                GpiSetLineWidth(hps, 0x10000);
            }
        }
    }
    // The processors along the bottom.
    if f & F_HLABELS != 0 {
        let lb = RectL::new(g.l, 2, g.r, g.b - 2);
        gpi::frame(hps, lb, edge);
        let row = lb.t - lay.fh - 2 + lay.desc;
        if f & F_BAR != 0 {
            for i in 0..=n {
                let (l, r) = bar_span(&g, n, i);
                let s = if i < n { number(i + 1) } else { b"Average".to_vec() };
                let sw = gpi::text_width(hps, &s);
                gpi::text_at(hps, (l + r - sw) / 2, row, &s, text);
            }
        } else {
            let slot = lb.width() / n as i32;
            let sw = (slot / 3).clamp(6, 120);
            for i in 0..n {
                let s = alloc::format!("{} - ", i + 1).into_bytes();
                let tw = gpi::text_width(hps, &s);
                let x = lb.l + slot * i as i32 + (slot - tw - sw) / 2;
                gpi::text_at(hps, x, row, &s, text);
                gpi::fill(hps, RectL::new(x + tw, row - lay.desc, x + tw + sw, row - lay.desc + lay.fh - 1), color_of(i));
            }
        }
        let t: &[u8] = b"Processor";
        let tw = gpi::text_width(hps, t);
        gpi::text_at(hps, (lb.l + lb.r - tw) / 2, lb.b + 2 + lay.desc, t, text);
    }
}

fn paint(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, 0, &mut upd) };
    let r = window_rect(hwnd);
    let (w, h) = (r.width().max(1), r.height().max(1));
    if a.back.as_ref().is_none_or(|b| b.cx != w || b.cy != h) {
        a.back = gpi::MemPs::new(a.hab, w, h);
    }
    match &a.back {
        Some(b) => {
            draw(b.hps, w, h);
            b.blit_to(hps, 0, 0);
        }
        None => {
            gpi::rgb_mode(hps);
            draw(hps, w, h);
        }
    }
    unsafe { WinEndPaint(hps) };
}

/// The current history (for hit testing): frozen or live.
fn shown() -> Vec<Vec<u8>> {
    let a = app();
    match (&a.frozen, &a.perf) {
        (Some(f), _) => f.clone(),
        (None, Some(p)) => p.history.clone(),
        _ => Vec::new(),
    }
}

/// A colour dropped on the window: the processor's graph under the
/// pointer gets it, else the background.
fn color_dropped(hwnd: u32, which: u32) {
    let a = app();
    let mut rgb = 0u32;
    let mut found = 0u32;
    if unsafe { WinQueryPresParam(hwnd, which, 0, &mut found, 4, &mut rgb as *mut u32 as *mut u8, 1) } == 0 {
        return;
    }
    let idx = COLORS.iter().enumerate().min_by_key(|(_, c)| {
        let (r, g, b) = ((c.1 >> 16) & 255, (c.1 >> 8) & 255, c.1 & 255);
        let (r2, g2, b2) = (((rgb >> 16) & 255) as i32, ((rgb >> 8) & 255) as i32, (rgb & 255) as i32);
        (r - r2).abs() + (g - g2).abs() + (b - b2).abs()
    });
    let Some((idx, _)) = idx else { return };
    let mut p = PointL::default();
    unsafe {
        WinQueryPointerPos(HWND_DESKTOP, &mut p);
        WinMapWindowPoints(HWND_DESKTOP, hwnd, &mut p, 1);
    }
    let r = window_rect(hwnd);
    let hps = unsafe { WinGetPS(hwnd) };
    let lay = layout(hps, r.width(), r.height(), a.set.flags);
    unsafe { WinReleasePS(hps) };
    match hit(&lay.graph, &shown(), a.set.flags, p.x, p.y) {
        Some(i) => a.set.colors[i.min(16)] = idx as u8,
        None => a.set.background = idx as u8,
    }
    update_menu();
}

pub fn command(id: u16) {
    let a = app();
    match id {
        IDM_BAR | IDM_LINE => {
            a.set.flags = (a.set.flags & !(F_BAR | F_LINE)) | if id == IDM_BAR { F_BAR } else { F_LINE };
        }
        IDM_HLABELS => a.set.flags ^= F_HLABELS,
        IDM_VLABELS => a.set.flags ^= F_VLABELS,
        IDM_GRID => a.set.flags ^= F_GRID,
        IDM_FREEZE => {
            a.set.flags ^= F_FREEZE;
            a.frozen = if a.set.flags & F_FREEZE != 0 { a.perf.as_ref().map(|p| p.history.clone()) } else { None };
        }
        IDM_STATUS => dialogs::status(),
        IDM_SAMPLERATE => {
            if dialogs::sample_rate() {
                unsafe { WinStartTimer(a.hab, a.client, TIMER_ID, a.set.rate) };
            }
        }
        x if x > IDM_BACKGROUND && x <= IDM_BACKGROUND + 16 => a.set.background = (x - IDM_BACKGROUND - 1) as u8,
        x if x > IDM_CPUCOLOR && x <= IDM_CPUCOLOR + 16 => dialogs::cpu_color((x - IDM_CPUCOLOR - 1) as usize),
        IDM_HELPINDEX => help_request(HM_HELP_INDEX),
        IDM_GENERALHELP => help_request(HM_EXT_HELP),
        IDM_USINGHELP => help_request(HM_DISPLAY_HELP),
        IDM_KEYSHELP => help_request(HM_KEYS_HELP),
        IDM_PRODUCT => product_info(a.frame, DLG_PRODUCT, b"CPU Monitor - Product Information", b"CPU Monitor (Pulse)", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Shows how busy each processor is."]),
        _ => return,
    }
    update_menu();
    invalidate(a.client);
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBACKGROUND => 0,
        WM_TIMER if lo(mp1) == TIMER_ID => {
            if let Some(p) = a.perf.as_mut() {
                p.sample();
            }
            if a.frozen.is_none() {
                invalidate(hwnd);
            }
            0
        }
        WM_COMMAND => {
            command(lo(mp1) as u16);
            0
        }
        WM_BUTTON1DBLCLK => {
            show_controls(a.bare);
            0
        }
        WM_BUTTON2UP | WM_CONTEXTMENU => {
            if msg == WM_CONTEXTMENU && hi(mp2) == 0 {
                // The mouse's comes as WM_BUTTON2UP.
                return 0;
            }
            let (x, y) = if msg == WM_BUTTON2UP { mouse_xy(mp1) } else { (20, 20) };
            let mut p = pt(x, y);
            unsafe { WinMapWindowPoints(hwnd, HWND_DESKTOP, &mut p, 1) };
            let m = popup_menu(hwnd, &popup_items());
            unsafe { WinPopupMenu(HWND_DESKTOP, hwnd, m, p.x, p.y, 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2) };
            1
        }
        WM_PRESPARAMCHANGED => {
            if mp1 == PP_BACKGROUNDCOLOR || mp1 == PP_FOREGROUNDCOLOR {
                color_dropped(hwnd, mp1);
            }
            invalidate(hwnd);
            0
        }
        WM_SIZE => {
            invalidate(hwnd);
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
