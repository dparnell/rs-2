//! PMVIOWIN.EXE: the PM side of windowed VIO sessions ("OS/2 Window").
//!
//! The kernel starts this program when the first windowed session
//! appears (or it runs as the protected shell with /SHELL). It owns one
//! frame window per windowable session and never waits for a session:
//!
//! - a watcher thread waits in the kernel (RS2API `Rs2VioWait`) until a
//!   windowed screen or the session list changes and posts one message
//!   to the window thread, which reads the screens (`Rs2VioReadScreen`)
//!   and invalidates what changed; a session whose programs hang or spin
//!   just stops changing;
//! - painting renders the cells with the session's font and palette
//!   (crate `viocell`) and draws them with GpiDrawBits;
//! - keys reach the session from PM's input thread directly (the client
//!   is registered with `Rs2PmVioWindow`); mouse events go to sessions
//!   that opened the mouse (`Rs2VioMouse`), otherwise the mouse marks
//!   text;
//! - the system menu has Mark, Copy, Copy All, Paste, Fullscreen and the
//!   font sizes; Close asks before it ends the session's programs.
//!
//! Usage: `PMVIOWIN [/SHELL]`: with /SHELL it starts a command prompt in
//! a window when there is none and keeps running when the last window
//! closes (a desktop to boot into); otherwise it ends with the last
//! windowed session.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use viocell::{Block, Cursor, Font, Grid, Style};

os2::entry!(main);

// ---- PM ----

#[repr(C)]
#[derive(Default)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RectL {
    l: i32,
    b: i32,
    r: i32,
    t: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PointL {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    handle: u32,
}

type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: *const u8, text: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.765"]
    fn WinInvalidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.877"]
    fn WinSetWindowText(h: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.884"]
    fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.883"]
    fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.851"]
    fn WinSetActiveWindow(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.860"]
    fn WinSetFocus(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.852"]
    fn WinSetCapture(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.829"]
    fn WinQuerySysValue(desktop: u32, i: i32) -> i32;
    #[link_name = "PMWIN.789"]
    fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.735"]
    fn WinEnableWindow(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.793"]
    fn WinOpenClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.707"]
    fn WinCloseClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.733"]
    fn WinEmptyClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.854"]
    fn WinSetClipbrdData(hab: u32, data: u32, fmt: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.806"]
    fn WinQueryClipbrdData(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.701"]
    fn WinAlarm(desktop: u32, kind: u32) -> u32;
    #[link_name = "PMGPI.603"]
    fn GpiDrawBits(hps: u32, bits: *const u8, bmi: *const u8, n: i32, pts: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMWIN.9002"]
    fn Rs2PmVioWindow(hwnd: u32, sid: u32, fl: u32) -> u32;
}

// ---- RS2API (kernel/src/api/viowin.rs) ----

/// INPUTEVENT
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Input {
    kind: u16,
    flags: u16,
    a: u32,
    b: u32,
    time: u32,
}

/// RS2VIOSESSION
#[repr(C)]
#[derive(Clone, Copy)]
struct VioSession {
    sid: u32,
    flags: u32,
    changes: u32,
    leader: u32,
    title: [u8; 64],
}

const VS_WINDOWED: u32 = 0x01;
const VS_ACTIVATE: u32 = 0x02;

const ST_CURSOR: u32 = 0x01;
const ST_BLINK: u32 = 0x02;
const ST_MOUSE: u32 = 0x08;
/// A DOS session in a graphics mode (Rs2VioReadPixels).
const ST_PIXELS: u32 = 0x20;
/// A DOS session: the mouse goes to it as positions (Rs2VdmMouse).
const ST_DOS: u32 = 0x40;

const CTL_FOCUS: u32 = 1;
const CTL_WINDOWED: u32 = 2;
const CTL_STOP: u32 = 3;
const CTL_REGISTER: u32 = 5;

unsafe extern "C" {
    #[link_name = "RS2API.10"]
    fn Rs2VioWait(pul_gen: *mut u32, ms: u32) -> u32;
    #[link_name = "RS2API.11"]
    fn Rs2VioQuerySessions(e: *mut VioSession, max: u32, found: *mut u32, all: u32) -> u32;
    #[link_name = "RS2API.12"]
    fn Rs2VioReadScreen(sid: u32, state: *mut u32, cells: *mut u16, cb: u32) -> u32;
    #[link_name = "RS2API.13"]
    fn Rs2VioQueryFont(sid: u32, cy: u32, glyphs: *mut u8, cb: u32) -> u32;
    #[link_name = "RS2API.14"]
    fn Rs2VioKey(sid: u32, ev: *const Input) -> u32;
    #[link_name = "RS2API.15"]
    fn Rs2VioMouse(sid: u32, row: i32, col: i32, buttons: u32) -> u32;
    #[link_name = "RS2API.404"]
    fn Rs2VioReadPixels(sid: u32, info: *mut u32, pixels: *mut u8, cb: u32, pal: *mut u32) -> u32;
    #[link_name = "RS2API.405"]
    fn Rs2VdmMouse(sid: u32, x: u32, y: u32, buttons: u32) -> u32;
    #[link_name = "RS2API.16"]
    fn Rs2VioControl(sid: u32, cmd: u32, arg: u32) -> u32;
}

// Messages and constants.
const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0007;
const WM_SETFOCUS: u32 = 0x000f;
const WM_COMMAND: u32 = 0x0020;
const WM_PAINT: u32 = 0x0023;
const WM_TIMER: u32 = 0x0024;
const WM_CLOSE: u32 = 0x0029;
const WM_QUIT: u32 = 0x002a;
const WM_VSCROLL: u32 = 0x0031;
const WM_HSCROLL: u32 = 0x0032;
const WM_MOUSEMOVE: u32 = 0x0070;
const WM_BUTTON1DOWN: u32 = 0x0071;
const WM_BUTTON1UP: u32 = 0x0072;
const WM_BUTTON2DOWN: u32 = 0x0074;
const WM_BUTTON2UP: u32 = 0x0075;
const WM_BUTTON3DOWN: u32 = 0x0077;
const WM_BUTTON3UP: u32 = 0x0078;
const WM_CHAR: u32 = 0x007a;
/// From the watcher thread: something changed.
const WM_SYNC: u32 = 0x1000;

const MM_INSERTITEM: u32 = 0x0180;
const MM_QUERYITEM: u32 = 0x0182;
const MM_SETITEMATTR: u32 = 0x0192;
const SBM_SETSCROLLBAR: u32 = 0x01a0;
const SBM_SETTHUMBSIZE: u32 = 0x01a6;

const HWND_DESKTOP: u32 = 1;
const HWND_OBJECT: u32 = 2;
const FID_SYSMENU: u32 = 0x8002;
const FID_VERTSCROLL: u32 = 0x8006;
const FID_HORZSCROLL: u32 = 0x8007;
const SC_SYSMENU: u32 = 0x8007;

const MIS_TEXT: u16 = 0x0001;
const MIS_SEPARATOR: u16 = 0x0004;
const MIA_CHECKED: u16 = 0x2000;
const MIA_DISABLED: u16 = 0x4000;

const KC_CHAR: u32 = 0x0001;
const KC_VIRTUALKEY: u32 = 0x0002;
const KC_SHIFT: u32 = 0x0008;
const KC_CTRL: u32 = 0x0010;
const KC_ALT: u32 = 0x0020;
const KC_KEYUP: u32 = 0x0040;
const VK_NEWLINE: u32 = 0x08;
const VK_ESC: u32 = 0x0f;
const VK_LEFT: u32 = 0x15;
const VK_UP: u32 = 0x16;
const VK_RIGHT: u32 = 0x17;
const VK_DOWN: u32 = 0x18;
const VK_ENTER: u32 = 0x1e;

const SWP_SIZE: u32 = 0x0001;
const SWP_MOVE: u32 = 0x0002;

/// System menu items.
const IDM_MARK: u32 = 0x7f01;
const IDM_COPY: u32 = 0x7f02;
const IDM_COPYALL: u32 = 0x7f03;
const IDM_PASTE: u32 = 0x7f04;
const IDM_FULLSCREEN: u32 = 0x7f05;
const IDM_FONT: u32 = 0x7f10; // + height

/// Rs2PmVioWindow flags.
const VW_DIRECT: u32 = 0x01;
const VW_FULLSCREEN: u32 = 0x02;

const TID_BLINK: u32 = 1;
const CLR_BLACK: i32 = -1;

const MAX_CELLS: usize = 132 * 100;

// ---- Windows ----

struct Win {
    sid: u32,
    frame: u32,
    client: u32,
    title: [u8; 64],
    /// Screen generation last read.
    changes: u32,
    cols: usize,
    rows: usize,
    cells: Vec<u16>,
    cur: Option<(usize, usize)>,
    cur_lines: (usize, usize),
    cell_h: usize,
    codepage: u32,
    flags: u32,
    font_gen: u32,
    palette: [u32; 16],
    /// Font height chosen in the menu (None: the session's own).
    font_choice: Option<usize>,
    font_h: usize,
    font: Vec<u8>,
    /// What the font was loaded for: (height, code page, font generation).
    font_key: (usize, u32, u32),
    /// First visible column and row.
    sx: usize,
    sy: usize,
    mark: Option<Block>,
    /// Mouse marking in progress from this cell.
    anchor: Option<(usize, usize)>,
    /// Mark mode from the menu: keys mark text instead of reaching the
    /// session.
    mark_mode: bool,
    windowed: bool,
    focus: bool,
    buttons: u32,
    /// A DOS session's graphics picture: width, height, pixels, palette.
    pix: Option<(usize, usize, Vec<u8>, [u32; 256])>,
    pix_dims: (usize, usize),
}

static mut WINS: Vec<Win> = Vec::new();
static HAB: AtomicU32 = AtomicU32::new(0);
static OBJECT: AtomicU32 = AtomicU32::new(0);
static ACK: AtomicU32 = AtomicU32::new(0);
static BLINK_OFF: AtomicU32 = AtomicU32::new(0);
/// Windows have been made (the host ends with the last one unless /SHELL).
static HAD_WINDOWS: AtomicU32 = AtomicU32::new(0);
static SHELL: AtomicU32 = AtomicU32::new(0);
static CASCADE: AtomicU32 = AtomicU32::new(0);

fn wins() -> &'static mut Vec<Win> {
    unsafe { &mut *core::ptr::addr_of_mut!(WINS) }
}

fn by_client(h: u32) -> Option<usize> {
    wins().iter().position(|w| w.client == h)
}

fn by_sid(sid: u32) -> Option<usize> {
    wins().iter().position(|w| w.sid == sid)
}

/// How big a DOS graphics picture is shown: doubled across below 400
/// pixels, 200-line modes double scanned (as a VGA shows them).
fn pix_size(w: usize, h: usize) -> (i32, i32) {
    let sx = if w < 400 { 2 } else { 1 };
    let sy = if h <= 240 { 2 } else { 1 };
    ((w * sx) as i32, (h * sy) as i32)
}

impl Win {
    fn grid(&self) -> Grid<'_> {
        Grid { cells: &self.cells, cols: self.cols, rows: self.rows }
    }

    fn content_size(&self) -> (i32, i32) {
        if let Some((w, h, ..)) = &self.pix {
            return pix_size(*w, *h);
        }
        ((self.cols * viocell::CELL_W) as i32, (self.rows * self.font_h) as i32)
    }

    fn client_size(&self) -> (i32, i32) {
        let mut r = RectL::default();
        unsafe { WinQueryWindowRect(self.client, &mut r) };
        (r.r - r.l, r.t - r.b)
    }

    /// Visible columns and rows (whole cells).
    fn visible(&self) -> (usize, usize) {
        let (cw, ch) = self.client_size();
        ((cw.max(0) as usize / viocell::CELL_W).max(1), (ch.max(0) as usize / self.font_h.max(1)).max(1))
    }

    /// Client rectangle of cells rows r0..r1, columns c0..c1.
    fn cell_rect(&self, r0: usize, r1: usize, c0: usize, c1: usize) -> RectL {
        let (_, ch) = self.client_size();
        let x0 = (c0 as i32 - self.sx as i32) * viocell::CELL_W as i32;
        let x1 = (c1 as i32 - self.sx as i32) * viocell::CELL_W as i32;
        let top = ch - (r0 as i32 - self.sy as i32) * self.font_h as i32;
        let bot = ch - (r1 as i32 - self.sy as i32) * self.font_h as i32;
        RectL { l: x0, b: bot, r: x1, t: top }
    }

    fn invalidate_cells(&self, r0: usize, r1: usize, c0: usize, c1: usize) {
        let r = self.cell_rect(r0, r1, c0, c1);
        unsafe { WinInvalidateRect(self.client, &r, 0) };
    }

    fn invalidate_all(&self) {
        unsafe { WinInvalidateRect(self.client, null(), 0) };
    }

    /// The cell under client point (x, y), clamped to the screen.
    fn cell_at(&self, x: i32, y: i32) -> (usize, usize) {
        let (_, ch) = self.client_size();
        let row = ((ch - 1 - y).max(0) as usize / self.font_h.max(1) + self.sy).min(self.rows.saturating_sub(1));
        let col = (x.max(0) as usize / viocell::CELL_W + self.sx).min(self.cols.saturating_sub(1));
        (row, col)
    }

    fn register(&self) {
        let mut fl = if self.mark_mode { 0 } else { VW_DIRECT };
        if !self.windowed {
            fl |= VW_FULLSCREEN;
        }
        unsafe { Rs2PmVioWindow(self.client, self.sid, fl) };
    }
}

// ---- Reading sessions ----

/// Read a session's screen into its window; invalidate what changed.
fn refresh(i: usize) {
    let w = &mut wins()[i];
    let mut st = [0u32; 32];
    let mut cells: Vec<u16> = alloc::vec![0; MAX_CELLS];
    if unsafe { Rs2VioReadScreen(w.sid, st.as_mut_ptr(), cells.as_mut_ptr(), (MAX_CELLS * 2) as u32) } != 0 {
        return;
    }
    if st[9] & ST_PIXELS != 0 {
        // A DOS session in a graphics mode: the picture instead of cells.
        let mut info = [0u32; 4];
        let mut pal = [0u32; 256];
        // The previous picture's buffer, unless the size changed.
        let mut px: Vec<u8> = match w.pix.take() {
            Some((.., d, _)) => d,
            None => Vec::new(),
        };
        let guess = (w.pix_dims.0 * w.pix_dims.1).max(320 * 200);
        px.resize(guess, 0);
        let mut rc = unsafe { Rs2VioReadPixels(w.sid, info.as_mut_ptr(), px.as_mut_ptr(), px.len() as u32, pal.as_mut_ptr()) };
        if rc == 0 && (info[0] * info[1]) as usize > px.len() {
            // A bigger mode than last time: again with room for it.
            px.resize((info[0] * info[1]) as usize, 0);
            rc = unsafe { Rs2VioReadPixels(w.sid, info.as_mut_ptr(), px.as_mut_ptr(), px.len() as u32, pal.as_mut_ptr()) };
        }
        if rc == 0 {
            let (pw, ph) = (info[0] as usize, info[1] as usize);
            px.truncate(pw * ph);
            let resized = (pw, ph) != w.pix_dims;
            w.pix_dims = (pw, ph);
            w.pix = Some((pw, ph, px, pal));
            w.changes = st[0];
            w.flags = st[9];
            if resized {
                fit_window(i);
            } else {
                wins()[i].invalidate_all();
            }
            return;
        }
    }
    let had_pix = w.pix.take().is_some() || core::mem::take(&mut w.pix_dims) != (0, 0);
    let (cols, rows) = (st[1] as usize, st[2] as usize);
    cells.truncate(cols * rows);
    let cur = (st[9] & ST_CURSOR != 0).then_some((st[3] as usize, st[4] as usize));
    let cell_h = st[7] as usize;
    let mut palette = [0u32; 16];
    palette.copy_from_slice(&st[11..27]);
    w.changes = st[0];
    let mode_changed = cols != w.cols || rows != w.rows || cell_h != w.cell_h;
    let mut all = mode_changed || had_pix || palette != w.palette || (st[9] ^ w.flags) & ST_BLINK != 0 || st[10] != w.font_gen || st[8] != w.codepage;
    let old_cur = w.cur;
    w.cols = cols;
    w.rows = rows;
    w.cell_h = cell_h;
    w.codepage = st[8];
    w.font_gen = st[10];
    w.flags = st[9];
    w.palette = palette;
    w.cur = cur;
    w.cur_lines = (st[5] as usize, st[6] as usize);
    if load_font(i) {
        all = true;
    }
    let w = &mut wins()[i];
    if all {
        w.cells = cells;
    } else {
        // Rows whose cells changed, as column spans.
        for r in 0..rows {
            let row = &cells[r * cols..r * cols + cols];
            let old = &w.cells[r * cols..r * cols + cols];
            if row == old {
                continue;
            }
            let first = (0..cols).find(|&c| row[c] != old[c]).unwrap_or(0);
            let last = (0..cols).rev().find(|&c| row[c] != old[c]).unwrap_or(cols - 1);
            w.invalidate_cells(r, r + 1, first, last + 1);
        }
        w.cells = cells;
        if old_cur != cur || cur.is_some() {
            for (r, c) in [old_cur, cur].into_iter().flatten() {
                w.invalidate_cells(r, r + 1, c, c + 1);
            }
        }
    }
    if mode_changed || had_pix {
        fit_window(i);
    } else if all {
        wins()[i].invalidate_all();
    }
}

/// Load the glyphs for the window's font height. True if they changed.
fn load_font(i: usize) -> bool {
    let w = &mut wins()[i];
    let h = w.font_choice.unwrap_or(w.cell_h);
    let h = if matches!(h, 8 | 14 | 16) { h } else { 16 };
    let key = (h, w.codepage, w.font_gen);
    if key == w.font_key && !w.font.is_empty() {
        return false;
    }
    let mut f = alloc::vec![0u8; 256 * h];
    if unsafe { Rs2VioQueryFont(w.sid, h as u32, f.as_mut_ptr(), f.len() as u32) } != 0 {
        return false;
    }
    w.font = f;
    w.font_h = h;
    w.font_key = key;
    true
}

/// Size the frame to show the whole screen if it fits the display,
/// otherwise as much as fits (with scroll bars).
fn fit_window(i: usize) {
    let (frame, client, (cw, ch)) = {
        let w = &wins()[i];
        (w.frame, w.client, w.content_size())
    };
    let mut fr = RectL::default();
    let mut cr = RectL::default();
    unsafe {
        WinQueryWindowRect(frame, &mut fr);
        WinQueryWindowRect(client, &mut cr);
    }
    let (dx, dy) = ((fr.r - fr.l) - (cr.r - cr.l), (fr.t - fr.b) - (cr.t - cr.b));
    let (sw, sh) = unsafe { (WinQuerySysValue(HWND_DESKTOP, 20), WinQuerySysValue(HWND_DESKTOP, 21)) };
    let (w_, h_) = ((cw + dx).min(sw), (ch + dy).min(sh));
    // Keep the top left corner where it is, inside the screen.
    let mut pos = RectL::default();
    frame_pos(frame, &mut pos);
    let top = pos.t.min(sh);
    let x = pos.l.min(sw - w_).max(0);
    let y = (top - h_).max(0);
    unsafe { WinSetWindowPos(frame, 0, x, y, w_, h_, SWP_SIZE | SWP_MOVE) };
    {
        let w = &mut wins()[i];
        w.sx = 0;
        w.sy = 0;
    }
    update_scrollbars(i);
    wins()[i].invalidate_all();
}

/// The frame's rectangle on the desktop.
fn frame_pos(frame: u32, r: &mut RectL) {
    #[repr(C)]
    #[derive(Default)]
    struct Swp {
        fl: u32,
        cy: i32,
        cx: i32,
        y: i32,
        x: i32,
        behind: u32,
        hwnd: u32,
        reserved: [u32; 2],
    }
    unsafe extern "C" {
        #[link_name = "PMWIN.837"]
        fn WinQueryWindowPos(h: u32, swp: *mut Swp) -> u32;
    }
    let mut s = Swp::default();
    unsafe { WinQueryWindowPos(frame, &mut s) };
    *r = RectL { l: s.x, b: s.y, r: s.x + s.cx, t: s.y + s.cy };
}

fn update_scrollbars(i: usize) {
    let (frame, cols, rows, (vc, vr), sx, sy) = {
        let w = &wins()[i];
        (w.frame, w.cols, w.rows, w.visible(), w.sx, w.sy)
    };
    let vs = unsafe { WinWindowFromID(frame, FID_VERTSCROLL) };
    let hs = unsafe { WinWindowFromID(frame, FID_HORZSCROLL) };
    let max_r = rows.saturating_sub(vr);
    let max_c = cols.saturating_sub(vc);
    unsafe {
        WinSendMsg(vs, SBM_SETSCROLLBAR, sy as u32, (max_r as u32) << 16);
        WinSendMsg(vs, SBM_SETTHUMBSIZE, (vr as u32) | (rows as u32) << 16, 0);
        WinEnableWindow(vs, (max_r > 0) as u32);
        WinSendMsg(hs, SBM_SETSCROLLBAR, sx as u32, (max_c as u32) << 16);
        WinSendMsg(hs, SBM_SETTHUMBSIZE, (vc as u32) | (cols as u32) << 16, 0);
        WinEnableWindow(hs, (max_c > 0) as u32);
    }
}

fn scroll_to(i: usize, sx: usize, sy: usize) {
    let w = &mut wins()[i];
    let (vc, vr) = w.visible();
    let sx = sx.min(w.cols.saturating_sub(vc));
    let sy = sy.min(w.rows.saturating_sub(vr));
    if (sx, sy) == (w.sx, w.sy) {
        return;
    }
    w.sx = sx;
    w.sy = sy;
    w.invalidate_all();
    update_scrollbars(i);
}

/// Bring the windows in line with the kernel's windowable sessions.
fn sync() {
    let mut list = [VioSession { sid: 0, flags: 0, changes: 0, leader: 0, title: [0; 64] }; 32];
    let mut found = 0;
    unsafe { Rs2VioQuerySessions(list.as_mut_ptr(), 32, &mut found, 0) };
    let list = &list[..found.min(32) as usize];
    // Windows of sessions that ended.
    let gone: Vec<u32> = wins().iter().filter(|w| !list.iter().any(|s| s.sid == w.sid)).map(|w| w.frame).collect();
    for f in gone {
        wins().retain(|w| w.frame != f);
        unsafe { WinDestroyWindow(f) };
    }
    for s in list {
        let i = match by_sid(s.sid) {
            Some(i) => i,
            None => match create_window(s) {
                Some(i) => i,
                None => continue,
            },
        };
        if wins()[i].title != s.title {
            let w = &mut wins()[i];
            w.title = s.title;
            let mut t = s.title;
            t[63] = 0;
            unsafe { WinSetWindowText(w.frame, t.as_ptr()) };
        }
        let windowed = s.flags & VS_WINDOWED != 0;
        if wins()[i].windowed != windowed {
            let w = &mut wins()[i];
            w.windowed = windowed;
            w.register();
            unsafe { WinShowWindow(w.frame, windowed as u32) };
        }
        if wins()[i].changes != s.changes {
            refresh(i);
        }
        if s.flags & VS_ACTIVATE != 0 && windowed {
            let w = &wins()[i];
            unsafe {
                WinShowWindow(w.frame, 1);
                WinSetActiveWindow(HWND_DESKTOP, w.frame);
            }
        }
    }
    if wins().is_empty() && HAD_WINDOWS.load(Ordering::Relaxed) != 0 && SHELL.load(Ordering::Relaxed) == 0 {
        unsafe { WinPostMsg(OBJECT.load(Ordering::Relaxed), WM_QUIT, 0, 0) };
    }
}

fn create_window(s: &VioSession) -> Option<usize> {
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX |
    // FCF_VERTSCROLL | FCF_HORZSCROLL | FCF_TASKLIST
    let mut fcf: u32 = 0x1 | 0x2 | 0x8 | 0x30 | 0x40 | 0x80 | 0x800;
    let mut client = 0;
    let mut t = s.title;
    t[63] = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"Rs2VioWindow\0".as_ptr(), t.as_ptr(), 0, 0, s.sid, &mut client) };
    if frame == 0 || client == 0 {
        return None;
    }
    wins().push(Win {
        sid: s.sid,
        frame,
        client,
        title: s.title,
        changes: s.changes.wrapping_sub(1),
        cols: 0,
        rows: 0,
        cells: Vec::new(),
        cur: None,
        cur_lines: (14, 15),
        cell_h: 16,
        codepage: 0,
        flags: 0,
        font_gen: 0,
        palette: [0; 16],
        font_choice: None,
        font_h: 16,
        font: Vec::new(),
        font_key: (0, 0, 0),
        sx: 0,
        sy: 0,
        mark: None,
        anchor: None,
        mark_mode: false,
        windowed: s.flags & VS_WINDOWED != 0,
        focus: false,
        buttons: 0,
        pix: None,
        pix_dims: (0, 0),
    });
    HAD_WINDOWS.store(1, Ordering::Relaxed);
    let i = wins().len() - 1;
    add_menu_items(frame);
    // A cascade position, then the size of the screen it shows.
    let n = CASCADE.fetch_add(1, Ordering::Relaxed) % 8;
    let sh = unsafe { WinQuerySysValue(HWND_DESKTOP, 21) };
    unsafe { WinSetWindowPos(frame, 0, 24 + 24 * n as i32, 0, 400, sh - 24 - 24 * n as i32, SWP_SIZE | SWP_MOVE) };
    refresh(i);
    let w = &wins()[i];
    w.register();
    if w.windowed {
        unsafe { WinShowWindow(frame, 1) };
    }
    Some(i)
}

/// Mark, Copy, Copy All, Paste, Fullscreen and the font sizes in the
/// system menu.
fn add_menu_items(frame: u32) {
    let sys = unsafe { WinWindowFromID(frame, FID_SYSMENU) };
    let mut mi = MenuItem::default();
    if unsafe { WinSendMsg(sys, MM_QUERYITEM, SC_SYSMENU, &mut mi as *mut MenuItem as u32) } == 0 || mi.sub == 0 {
        return;
    }
    let sub = mi.sub;
    let add = |style: u16, id: u32, text: &[u8]| {
        let item = MenuItem { pos: -1, style, attr: 0, id: id as u16, sub: 0, handle: 0 };
        unsafe { WinSendMsg(sub, MM_INSERTITEM, &item as *const MenuItem as u32, text.as_ptr() as u32) };
    };
    add(MIS_SEPARATOR, 0xffff, b"\0");
    add(MIS_TEXT, IDM_MARK, b"Mar~k\0");
    add(MIS_TEXT, IDM_COPY, b"Cop~y\0");
    add(MIS_TEXT, IDM_COPYALL, b"Copy ~All\0");
    add(MIS_TEXT, IDM_PASTE, b"~Paste\0");
    add(MIS_SEPARATOR, 0xffff, b"\0");
    add(MIS_TEXT, IDM_FULLSCREEN, b"~Fullscreen\tAlt+Home\0");
    add(MIS_SEPARATOR, 0xffff, b"\0");
    add(MIS_TEXT, IDM_FONT + 16, b"Font 8 x 1~6\0");
    add(MIS_TEXT, IDM_FONT + 14, b"Font 8 x 1~4\0");
    add(MIS_TEXT, IDM_FONT + 8, b"Font 8 x ~8\0");
}

fn set_item_attr(frame: u32, id: u32, mask: u16, on: bool) {
    let sys = unsafe { WinWindowFromID(frame, FID_SYSMENU) };
    let v = if on { mask } else { 0 };
    unsafe { WinSendMsg(sys, MM_SETITEMATTR, id | 1 << 16, mask as u32 | (v as u32) << 16) };
}

fn update_menu(i: usize) {
    let w = &wins()[i];
    let has_mark = w.mark.is_some();
    set_item_attr(w.frame, IDM_COPY, MIA_DISABLED, !has_mark);
    set_item_attr(w.frame, IDM_MARK, MIA_CHECKED, w.mark_mode);
    for h in [8, 14, 16] {
        set_item_attr(w.frame, IDM_FONT + h as u32, MIA_CHECKED, w.font_h == h);
    }
}

// ---- Painting ----

fn paint(i: usize) {
    let w = &wins()[i];
    let mut r = RectL::default();
    let hps = unsafe { WinBeginPaint(w.client, 0, &mut r) };
    if let Some((pw, ph, px, pal)) = &w.pix {
        // The whole picture as a bottom-up 24-bit bitmap, stretched to
        // its shown size.
        let (cw, ch) = w.client_size();
        let (dw, dh) = pix_size(*pw, *ph);
        let stride = viocell::bgr24_stride(*pw);
        let mut buf = alloc::vec![0u8; stride * ph];
        for y in 0..*ph {
            let row = &px[y * pw..y * pw + pw];
            let out = &mut buf[(ph - 1 - y) * stride..];
            for (x, &c) in row.iter().enumerate() {
                let v = pal[c as usize];
                out[x * 3] = v as u8;
                out[x * 3 + 1] = (v >> 8) as u8;
                out[x * 3 + 2] = (v >> 16) as u8;
            }
        }
        let mut bmi = [0u8; 12];
        bmi[0] = 12;
        bmi[4..6].copy_from_slice(&(*pw as u16).to_le_bytes());
        bmi[6..8].copy_from_slice(&(*ph as u16).to_le_bytes());
        bmi[8] = 1;
        bmi[10] = 24;
        let top = ch;
        let pts = [PointL { x: 0, y: top - dh }, PointL { x: dw - 1, y: top - 1 }, PointL { x: 0, y: 0 }, PointL { x: *pw as i32, y: *ph as i32 }];
        unsafe { GpiDrawBits(hps, buf.as_ptr(), bmi.as_ptr(), 4, pts.as_ptr(), 0xcc, 2) };
        if cw > dw {
            unsafe { WinFillRect(hps, &RectL { l: dw, b: 0, r: cw, t: ch }, CLR_BLACK) };
        }
        if ch > dh {
            unsafe { WinFillRect(hps, &RectL { l: 0, b: 0, r: dw.min(cw), t: ch - dh }, CLR_BLACK) };
        }
        unsafe { WinEndPaint(hps) };
        return;
    }
    let (cw, ch) = w.client_size();
    let (pw, ph) = w.content_size();
    // Outside the text: black.
    let x_end = pw - (w.sx * viocell::CELL_W) as i32;
    let y_end = ch - (ph - (w.sy * w.font_h) as i32);
    if r.r > x_end {
        unsafe { WinFillRect(hps, &RectL { l: x_end.max(r.l), b: r.b, r: r.r, t: r.t }, CLR_BLACK) };
    }
    if r.b < y_end {
        unsafe { WinFillRect(hps, &RectL { l: r.l, b: r.b, r: r.r, t: y_end.min(r.t) }, CLR_BLACK) };
    }
    if w.cols > 0 && w.rows > 0 && !w.font.is_empty() {
        let fh = w.font_h as i32;
        let cwid = viocell::CELL_W as i32;
        // Cells under the update rectangle.
        let r0 = (((ch - r.t).max(0)) / fh) as usize + w.sy;
        let r1 = ((((ch - r.b).max(0)) + fh - 1) / fh) as usize + w.sy;
        let c0 = (r.l.max(0) / cwid) as usize + w.sx;
        let c1 = ((r.r.max(0) + cwid - 1) / cwid) as usize + w.sx;
        let (r1, c1) = (r1.min(w.rows), c1.min(w.cols));
        if r0 < r1 && c0 < c1 {
            let width = (c1 - c0) * viocell::CELL_W;
            let height = (r1 - r0) * w.font_h;
            let stride = viocell::bgr24_stride(width);
            let mut buf = alloc::vec![0u8; stride * height];
            let blink_off = BLINK_OFF.load(Ordering::Relaxed) != 0;
            let cursor = w.cur.filter(|_| w.focus && !blink_off).map(|(row, col)| {
                let (a, b) = viocell::scale_lines(w.cur_lines.0, w.cur_lines.1, w.cell_h, w.font_h);
                Cursor { row, col, first: a, last: b }
            });
            let style = Style { palette: w.palette, blink: w.flags & ST_BLINK != 0, blink_off, cursor, mark: w.mark };
            let font = Font { glyphs: &w.font, height: w.font_h };
            viocell::render_bgr24(&w.grid(), &font, &style, r0..r1, c0..c1, &mut buf, stride, true);
            // BITMAPINFOHEADER (1.x): cbFix, cx, cy, planes, bit count.
            let mut bmi = [0u8; 12];
            bmi[0] = 12;
            bmi[4..6].copy_from_slice(&(width as u16).to_le_bytes());
            bmi[6..8].copy_from_slice(&(height as u16).to_le_bytes());
            bmi[8] = 1;
            bmi[10] = 24;
            let rc = w.cell_rect(r0, r1, c0, c1);
            let pts = [PointL { x: rc.l, y: rc.b }, PointL { x: rc.r - 1, y: rc.t - 1 }, PointL { x: 0, y: 0 }];
            unsafe { GpiDrawBits(hps, buf.as_ptr(), bmi.as_ptr(), 3, pts.as_ptr(), 0xcc, 0) };
        }
    }
    let _ = cw;
    unsafe { WinEndPaint(hps) };
}

/// Blink timer: the cursor and blinking characters.
fn blink() {
    BLINK_OFF.fetch_xor(1, Ordering::Relaxed);
    for w in wins().iter() {
        if !w.windowed {
            continue;
        }
        if let Some((r, c)) = w.cur.filter(|_| w.focus) {
            w.invalidate_cells(r, r + 1, c, c + 1);
        }
        if w.flags & ST_BLINK != 0 {
            for r in 0..w.rows {
                let row = &w.cells[r * w.cols..r * w.cols + w.cols];
                if let (Some(a), Some(b)) = (row.iter().position(|c| c & 0x8000 != 0), row.iter().rposition(|c| c & 0x8000 != 0)) {
                    w.invalidate_cells(r, r + 1, a, b + 1);
                }
            }
        }
    }
}

// ---- Clipboard ----

fn copy_text(text: &[u8]) {
    if text.is_empty() {
        return;
    }
    let hab = HAB.load(Ordering::Relaxed);
    let mut p: *mut u8 = null_mut();
    // PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE
    if unsafe { os2::DosAllocSharedMem(&mut p, null(), text.len() as u32 + 1, 0x1 | 0x2 | 0x10 | 0x200) } != 0 {
        return;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), p, text.len());
        *p.add(text.len()) = 0;
        if WinOpenClipbrd(hab) != 0 {
            WinEmptyClipbrd(hab);
            // CF_TEXT, CFI_POINTER
            WinSetClipbrdData(hab, p as u32, 1, 0x0400);
            WinCloseClipbrd(hab);
        }
    }
}

fn paste(sid: u32) {
    let hab = HAB.load(Ordering::Relaxed);
    let mut text = Vec::new();
    unsafe {
        if WinOpenClipbrd(hab) == 0 {
            return;
        }
        let p = WinQueryClipbrdData(hab, 1) as *const u8;
        if !p.is_null() {
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            text.extend_from_slice(core::slice::from_raw_parts(p, n));
        }
        WinCloseClipbrd(hab);
    }
    for (ch, scan) in viocell::paste_keys(&text) {
        let ev = Input { kind: 1, flags: 0, a: ch as u32 | (scan as u32) << 8 | 0x40 << 16, b: 0, time: 0 };
        unsafe { Rs2VioKey(sid, &ev) };
    }
}

// ---- Commands and input ----

fn set_mark(i: usize, m: Option<Block>) {
    let w = &mut wins()[i];
    if w.mark != m {
        w.mark = m;
        w.invalidate_all();
    }
    update_menu(i);
}

fn end_mark_mode(i: usize) {
    let w = &mut wins()[i];
    w.mark_mode = false;
    w.register();
    update_menu(i);
}

fn command(i: usize, id: u32) {
    let sid = wins()[i].sid;
    match id {
        IDM_MARK => {
            let w = &mut wins()[i];
            w.mark_mode = !w.mark_mode;
            w.register();
            let start = w.cur.unwrap_or((0, 0));
            let mm = w.mark_mode;
            set_mark(i, if mm { Some(Block::spanning(start, start)) } else { None });
        }
        IDM_COPY => {
            let w = &wins()[i];
            if let Some(m) = w.mark {
                let t = viocell::block_text(&w.grid(), m);
                copy_text(&t);
            }
            set_mark(i, None);
            end_mark_mode(i);
        }
        IDM_COPYALL => {
            let t = viocell::all_text(&wins()[i].grid());
            copy_text(&t);
        }
        IDM_PASTE => paste(sid),
        IDM_FULLSCREEN => {
            unsafe { Rs2VioControl(sid, CTL_WINDOWED, 0) };
        }
        id if (IDM_FONT + 8..=IDM_FONT + 16).contains(&id) => {
            wins()[i].font_choice = Some((id - IDM_FONT) as usize);
            load_font(i);
            fit_window(i);
            update_menu(i);
        }
        _ => {}
    }
}

/// A WM_CHAR the window got itself (mark mode, or before the
/// registration took effect).
fn char_msg(i: usize, mp1: u32, mp2: u32) -> u32 {
    let fs = mp1 & 0xffff;
    let up = fs & KC_KEYUP != 0;
    let vk = if fs & KC_VIRTUALKEY != 0 { mp2 >> 16 } else { 0 };
    if wins()[i].mark_mode {
        if up {
            return 1;
        }
        let w = &wins()[i];
        let m = w.mark.unwrap_or(Block::spanning((0, 0), (0, 0)));
        let (rows, cols) = (w.rows, w.cols);
        let shift = fs & KC_SHIFT != 0;
        let mv = |r: usize, c: usize, dr: i32, dc: i32| ((r as i32 + dr).clamp(0, rows as i32 - 1) as usize, (c as i32 + dc).clamp(0, cols as i32 - 1) as usize);
        let (dr, dc) = match vk {
            VK_LEFT => (0, -1),
            VK_RIGHT => (0, 1),
            VK_UP => (-1, 0),
            VK_DOWN => (1, 0),
            VK_ESC => {
                set_mark(i, None);
                end_mark_mode(i);
                return 1;
            }
            VK_NEWLINE | VK_ENTER => {
                command(i, IDM_COPY);
                return 1;
            }
            _ => return 1,
        };
        let nm = if shift {
            let (r, c) = mv(m.bottom, m.right, dr, dc);
            Block { bottom: r.max(m.top), right: c.max(m.left), ..m }
        } else {
            let (r, c) = mv(m.top, m.left, dr, dc);
            Block::spanning((r, c), (r, c))
        };
        set_mark(i, Some(nm));
        return 1;
    }
    // Pass it on to the session.
    let mut ch = if fs & KC_CHAR != 0 { mp2 & 0xff } else { 0 };
    let scan = mp1 >> 24;
    let mut shift = 0u32;
    if fs & KC_SHIFT != 0 {
        shift |= 0x02;
    }
    if fs & KC_CTRL != 0 {
        shift |= 0x04 | 0x100;
        if (ch as u8).is_ascii_alphabetic() {
            ch &= 0x1f;
        }
    }
    if fs & KC_ALT != 0 {
        shift |= 0x08 | 0x200;
        ch = 0;
    }
    let status = if up { 0 } else { 0x40 };
    let ev = Input { kind: 1, flags: shift as u16, a: ch | scan << 8 | status << 16 | (up as u32) << 24, b: 0, time: 0 };
    unsafe { Rs2VioKey(wins()[i].sid, &ev) };
    1
}

fn mouse(i: usize, msg: u32, mp1: u32) -> bool {
    let x = mp1 as i16 as i32;
    let y = (mp1 >> 16) as i16 as i32;
    let (cell, sid, uses_mouse, marking) = {
        let w = &wins()[i];
        (w.cell_at(x, y), w.sid, w.flags & ST_MOUSE != 0 && !w.mark_mode, w.anchor.is_some())
    };
    // DOS sessions get the pointer's position over the picture or text.
    if wins()[i].flags & ST_DOS != 0 && !wins()[i].mark_mode {
        let w = &mut wins()[i];
        match msg {
            WM_BUTTON1DOWN => w.buttons |= 1,
            WM_BUTTON1UP => w.buttons &= !1,
            WM_BUTTON2DOWN => w.buttons |= 2,
            WM_BUTTON2UP => w.buttons &= !2,
            WM_BUTTON3DOWN => w.buttons |= 4,
            WM_BUTTON3UP => w.buttons &= !4,
            _ => {}
        }
        let (_, ch) = w.client_size();
        let (pw, ph) = w.content_size();
        let fx = (x.clamp(0, pw.max(1) - 1) as u32 * 65536 / pw.max(1) as u32).min(65535);
        let fy = ((ch - 1 - y).clamp(0, ph.max(1) - 1) as u32 * 65536 / ph.max(1) as u32).min(65535);
        let (client, b) = (w.client, w.buttons);
        unsafe {
            if msg == WM_BUTTON1DOWN {
                WinSetFocus(HWND_DESKTOP, client);
            }
            WinSetCapture(HWND_DESKTOP, if b != 0 { client } else { 0 });
            Rs2VdmMouse(sid, fx, fy, b);
        }
        return msg != WM_MOUSEMOVE;
    }
    if uses_mouse {
        let w = &mut wins()[i];
        match msg {
            WM_BUTTON1DOWN => w.buttons |= 1,
            WM_BUTTON1UP => w.buttons &= !1,
            WM_BUTTON2DOWN => w.buttons |= 2,
            WM_BUTTON2UP => w.buttons &= !2,
            WM_BUTTON3DOWN => w.buttons |= 4,
            WM_BUTTON3UP => w.buttons &= !4,
            _ => {}
        }
        let client = w.client;
        let b = w.buttons;
        unsafe {
            WinSetCapture(HWND_DESKTOP, if b != 0 { client } else { 0 });
            Rs2VioMouse(sid, cell.0 as i32, cell.1 as i32, b);
        }
        return msg != WM_MOUSEMOVE;
    }
    match msg {
        WM_BUTTON1DOWN => {
            let client = wins()[i].client;
            wins()[i].anchor = Some(cell);
            set_mark(i, Some(Block::spanning(cell, cell)));
            unsafe {
                WinSetFocus(HWND_DESKTOP, client);
                WinSetCapture(HWND_DESKTOP, client);
            }
            true
        }
        WM_MOUSEMOVE if marking => {
            let a = wins()[i].anchor.unwrap_or(cell);
            set_mark(i, Some(Block::spanning(a, cell)));
            false
        }
        WM_BUTTON1UP if marking => {
            wins()[i].anchor = None;
            unsafe { WinSetCapture(HWND_DESKTOP, 0) };
            true
        }
        _ => false,
    }
}

fn close(i: usize) {
    let (sid, frame) = (wins()[i].sid, wins()[i].frame);
    let r = unsafe {
        WinMessageBox(
            HWND_DESKTOP,
            frame,
            b"Closing this window ends the programs running in it.\n\nDo you want to end them?\0".as_ptr(),
            b"Close\0".as_ptr(),
            0,
            // MB_OKCANCEL | MB_WARNING | MB_MOVEABLE
            0x0001 | 0x0020 | 0x4000,
        )
    };
    // MBID_OK
    if r == 1 {
        unsafe { Rs2VioControl(sid, CTL_STOP, 0) };
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(i) = by_client(h) else { return unsafe { WinDefWindowProc(h, msg, mp1, mp2) } };
    match msg {
        WM_PAINT => {
            paint(i);
            0
        }
        WM_SIZE => {
            update_scrollbars(i);
            let (sx, sy) = (wins()[i].sx, wins()[i].sy);
            scroll_to(i, sx, sy);
            0
        }
        WM_SETFOCUS => {
            let on = mp2 & 0xffff != 0;
            let w = &mut wins()[i];
            w.focus = on;
            unsafe { Rs2VioControl(w.sid, CTL_FOCUS, on as u32) };
            if let Some((r, c)) = w.cur {
                w.invalidate_cells(r, r + 1, c, c + 1);
            }
            0
        }
        WM_VSCROLL | WM_HSCROLL => {
            let (sx, sy, (vc, vr)) = (wins()[i].sx, wins()[i].sy, wins()[i].visible());
            let (pos, page) = if msg == WM_VSCROLL { (sy, vr) } else { (sx, vc) };
            let np = match mp2 >> 16 {
                1 => pos.saturating_sub(1),
                2 => pos + 1,
                3 => pos.saturating_sub(page),
                4 => pos + page,
                5 | 6 => (mp2 & 0xffff) as usize,
                _ => pos,
            };
            if msg == WM_VSCROLL { scroll_to(i, sx, np) } else { scroll_to(i, np, sy) }
            0
        }
        WM_COMMAND => {
            command(i, mp1 & 0xffff);
            0
        }
        WM_CHAR => char_msg(i, mp1, mp2),
        WM_MOUSEMOVE | WM_BUTTON1DOWN | WM_BUTTON1UP | WM_BUTTON2DOWN | WM_BUTTON2UP | WM_BUTTON3DOWN | WM_BUTTON3UP => {
            if mouse(i, msg, mp1) { 1 } else { unsafe { WinDefWindowProc(h, msg, mp1, mp2) } }
        }
        WM_CLOSE => {
            close(i);
            0
        }
        WM_DESTROY => {
            unsafe { Rs2PmVioWindow(h, 0, 0) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

extern "C" fn object_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_SYNC => {
            sync();
            unsafe { os2::DosPostEventSem(ACK.load(Ordering::Relaxed)) };
            0
        }
        WM_TIMER if mp1 == TID_BLINK => {
            blink();
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

// ---- The watcher thread ----

#[unsafe(naked)]
extern "C" fn watcher_entry(_arg: u32) -> u32 {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16",
        "call {f}",
        "mov esp, ebp", "pop ebp", "ret",
        f = sym watcher,
    );
}

extern "C" fn watcher() -> u32 {
    let mut seen = 0u32;
    let obj = OBJECT.load(Ordering::Relaxed);
    let ack = ACK.load(Ordering::Relaxed);
    loop {
        let rc = unsafe { Rs2VioWait(&mut seen, u32::MAX) };
        if rc != 0 && rc != 640 {
            unsafe { os2::DosSleep(100) };
            continue;
        }
        let mut n = 0;
        unsafe {
            os2::DosResetEventSem(ack, &mut n);
            if WinPostMsg(obj, WM_SYNC, 0, 0) == 0 {
                return 0;
            }
            // One update at a time, at most about 50 a second.
            os2::DosWaitEventSem(ack, 2000);
            os2::DosSleep(20);
        }
    }
}

// ---- Main ----

fn start_prompt() {
    let sd = os2::StartData {
        length: core::mem::size_of::<os2::StartData>() as u16,
        related: 0,
        fgbg: 0,
        trace: 0,
        title: b"OS/2 Window\0".as_ptr(),
        name: null(),
        inputs: b"\0".as_ptr(),
        termq: null(),
        env: null(),
        inherit: 0,
        session_type: 2,
    };
    let (mut sid, mut pid) = (0, 0);
    unsafe { os2::DosStartSession(&sd, &mut sid, &mut pid) };
}

fn main(s: &os2::Startup) -> u32 {
    let args = s.args();
    let shell = args.windows(6).any(|w| w.eq_ignore_ascii_case(b"/SHELL"));
    SHELL.store(shell as u32, Ordering::Relaxed);
    unsafe {
        let hab = WinInitialize(0);
        let hmq = WinCreateMsgQueue(hab, 0);
        if hab == 0 || hmq == 0 {
            return 1;
        }
        HAB.store(hab, Ordering::Relaxed);
        WinRegisterClass(hab, b"Rs2VioWindow\0".as_ptr(), client_proc, 0, 0);
        WinRegisterClass(hab, b"Rs2VioHost\0".as_ptr(), object_proc, 0, 0);
        let obj = WinCreateWindow(HWND_OBJECT, b"Rs2VioHost\0".as_ptr(), b"\0".as_ptr(), 0, 0, 0, 0, 0, 0, 3, 0, null(), null());
        OBJECT.store(obj, Ordering::Relaxed);
        Rs2VioControl(0, CTL_REGISTER, 0);
        let mut ack = 0;
        os2::DosCreateEventSem(null(), &mut ack, 0, 0);
        ACK.store(ack, Ordering::Relaxed);
        sync();
        if shell && wins().is_empty() {
            start_prompt();
        }
        let mut tid = 0;
        os2::DosCreateThread(&mut tid, core::mem::transmute::<extern "C" fn(u32) -> u32, os2::ThreadFn>(watcher_entry), 0, 2, 64 * 1024);
        WinStartTimer(hab, obj, TID_BLINK, 500);
        let mut q = QMsg::default();
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            WinDispatchMsg(hab, &q);
        }
        for w in wins().drain(..) {
            WinDestroyWindow(w.frame);
        }
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    let _ = WinAlarm;
    0
}
