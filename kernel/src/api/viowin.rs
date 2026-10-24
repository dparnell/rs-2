//! RS2API services for the PM host of windowed VIO sessions
//! (`PMVIOWIN.EXE`, crate::viowin). All 32-bit `_System`.
//!
//! | Ordinal | Call |
//! |---------|------|
//! | 10 | `Rs2VioWait(PULONG pulGen, ULONG ulTimeout)` |
//! | 11 | `Rs2VioQuerySessions(PRS2VIOSESSION aEntries, ULONG cMax, PULONG pcFound, ULONG fAll)` |
//! | 12 | `Rs2VioReadScreen(ULONG sid, PRS2VIOSTATE pState, PUSHORT pCells, ULONG cbCells)` |
//! | 13 | `Rs2VioQueryFont(ULONG sid, ULONG cy, PBYTE pGlyphs, ULONG cb)` |
//! | 14 | `Rs2VioKey(ULONG sid, PINPUTEVENT pKey)` |
//! | 15 | `Rs2VioMouse(ULONG sid, LONG row, LONG col, ULONG fsButtons)` |
//! | 16 | `Rs2VioControl(ULONG sid, ULONG cmd, ULONG arg)` |

use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::{err, Ctx};
use crate::{session, viowin};

const ERROR_TIMEOUT: u32 = 640;
const ERROR_SMG_SESSION_NOT_FOUND: u32 = 371;

/// RS2VIOSESSION: sid, flags, screen generation, leader pid, title[64].
const SESSION_SIZE: u32 = 80;
/// RS2VIOSESSION.flags
const VS_WINDOWED: u32 = 0x01;
const VS_ACTIVATE: u32 = 0x02;
const VS_WINDOWABLE: u32 = 0x04;
const VS_PM: u32 = 0x08;
const VS_FOREGROUND: u32 = 0x10;

/// RS2VIOSTATE.flags
const ST_CURSOR: u32 = 0x01;
const ST_BLINK: u32 = 0x02;
const ST_WINDOWED: u32 = 0x04;
const ST_MOUSE: u32 = 0x08;
const ST_USERFONT: u32 = 0x10;
/// A DOS session's graphics picture (Rs2VioReadPixels).
const ST_PIXELS: u32 = 0x20;
/// A DOS session: the mouse goes to it as positions (Rs2VdmMouse).
const ST_DOS: u32 = 0x40;

/// Rs2VioControl commands.
const CTL_FOCUS: u32 = 1;
const CTL_WINDOWED: u32 = 2;
const CTL_STOP: u32 = 3;
const CTL_SELECT: u32 = 4;
const CTL_REGISTER: u32 = 5;

/// Rs2VioWait(pulGen, ulTimeout): wait until something a host shows has
/// changed since generation *pulGen (ms, -1 = forever); stores the new
/// generation. The first call registers the caller as the host.
pub fn wait(c: &mut Ctx) -> u32 {
    let (p, ms) = (c.arg(0), c.arg(1));
    let Some(seen) = c.read_u32(p) else { return err::INVALID_ADDRESS };
    viowin::register_host(c.process.pid);
    let now = viowin::generation();
    if now != seen {
        c.write_u32(p, now);
        return 0;
    }
    if ms == 0 {
        return ERROR_TIMEOUT;
    }
    let deadline = (ms != u32::MAX).then(|| crate::irq::ticks() + ms as u64 * crate::irq::HZ / 1000);
    match viowin::wait(seen, deadline) {
        Some(g) => {
            c.write_u32(p, g);
            0
        }
        None if c.thread.exit_requested() => err::INTERRUPT,
        None => ERROR_TIMEOUT,
    }
}

/// Rs2VioQuerySessions(aEntries, cMax, pcFound, fAll): the windowable
/// sessions (fAll: every session but PM's) as RS2VIOSESSION entries;
/// *pcFound is the number there are (which may exceed cMax). A pending
/// activation request is reported once (VS_ACTIVATE).
pub fn query_sessions(c: &mut Ctx) -> u32 {
    let (at, max, pfound, all) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3) != 0);
    let activate = if all { 0 } else { viowin::take_activate() };
    let pm = session::pm_session();
    let fg = session::foreground();
    let mut list: Vec<(u32, u32, alloc::string::String)> = Vec::new();
    // The boot session, unless nothing runs there any more (a shell that
    // went to PM).
    let boot_used = super::signal::foreground_of(1).is_some_and(|p| !crate::display::is_pm(p.pid));
    if all && boot_used {
        list.push((1, 0, alloc::string::String::from("OS/2 Full Screen")));
    }
    for (sid, title, windowable, windowed) in session::list_full() {
        if !(windowable || all) || (sid == pm && sid != 0) {
            continue;
        }
        let mut f = 0;
        if windowable {
            f |= VS_WINDOWABLE;
        }
        if windowed {
            f |= VS_WINDOWED;
        }
        if sid == activate {
            f |= VS_ACTIVATE;
        }
        list.push((sid, f, title));
    }
    let n = list.len() as u32;
    for (i, (sid, mut f, title)) in list.into_iter().enumerate().take(max as usize) {
        if sid == fg {
            f |= VS_FOREGROUND;
        }
        if sid == pm {
            f |= VS_PM;
        }
        let changes = crate::screen::with_session(sid, |s| s.changes).unwrap_or(0);
        let leader = session::info(sid).map_or(0, |x| x.0);
        let mut e = Vec::with_capacity(SESSION_SIZE as usize);
        for v in [sid, f, changes, leader] {
            e.extend_from_slice(&v.to_le_bytes());
        }
        let mut t = [0u8; 64];
        let cp = crate::screen::with_session(sid, |s| s.codepage()).unwrap_or(crate::codepage::CodePage::Cp850);
        for (j, ch) in title.chars().take(63).enumerate() {
            t[j] = cp.from_char(ch).unwrap_or(b'?');
        }
        e.extend_from_slice(&t);
        if !c.write(at + i as u32 * SESSION_SIZE, &e) {
            return err::INVALID_ADDRESS;
        }
    }
    if pfound != 0 {
        c.write_u32(pfound, n);
    }
    0
}

/// Rs2VioReadScreen(sid, pState, pCells, cbCells): the state of a
/// session's screen (RS2VIOSTATE, 128 bytes) and, if pCells, its cells
/// (char | attr << 8, row by row, cols x rows of them as far as cbCells
/// holds).
///
/// RS2VIOSTATE: gen, cols, rows, cursor row, cursor column, first and
/// last cursor scan line (of cell_h), cell_h, code page, flags (ST_*),
/// font generation, then 16 colours 0x00RRGGBB (offset 44), 20 bytes
/// reserved.
pub fn read_screen(c: &mut Ctx) -> u32 {
    let (sid, pstate, pcells, cb) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if !session::exists(sid) {
        return ERROR_SMG_SESSION_NOT_FOUND;
    }
    let mouse = crate::mouse::is_open(sid);
    let dos = crate::vdm::is_dos(sid);
    let r = crate::screen::with_session(sid, |s| {
        let (cr, cc) = s.cursor().unwrap_or((s.cur_row, s.cur_col));
        let (a, b) = s.cursor_lines();
        let mut f = 0;
        if s.cursor().is_some() && s.cursor_type[3] != 0xffff {
            f |= ST_CURSOR;
        }
        if s.blink {
            f |= ST_BLINK;
        }
        if s.windowed {
            f |= ST_WINDOWED;
        }
        if mouse {
            f |= ST_MOUSE;
        }
        if s.user_font {
            f |= ST_USERFONT;
        }
        if s.pixmap.is_some() {
            f |= ST_PIXELS;
        }
        if dos {
            f |= ST_DOS;
        }
        let mut st = Vec::with_capacity(128);
        for v in [s.changes, s.cols as u32, s.rows as u32, cr as u32, cc as u32, a as u32, b as u32, s.cell_h as u32, s.codepage().number() as u32, f, s.font_gen] {
            st.extend_from_slice(&v.to_le_bytes());
        }
        for v in s.palette_rgb() {
            st.extend_from_slice(&v.to_le_bytes());
        }
        st.resize(128, 0);
        let cells = if pcells != 0 { s.cells_vec() } else { Vec::new() };
        (st, cells)
    });
    let Some((st, cells)) = r else { return ERROR_SMG_SESSION_NOT_FOUND };
    if pstate != 0 && !c.write(pstate, &st) {
        return err::INVALID_ADDRESS;
    }
    if pcells != 0 {
        let n = cells.len().min(cb as usize / 2);
        let mut b = Vec::with_capacity(n * 2);
        for v in &cells[..n] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        if !c.write(pcells, &b) {
            return err::INVALID_ADDRESS;
        }
    }
    0
}

/// Rs2VioQueryFont(sid, cy, pGlyphs, cb): the 256 glyphs (cy bytes each,
/// bit 7 = leftmost of 8 pixels) a session's text shows at cell height cy
/// (8, 14 or 16): its code page in the built-in fonts, or the font it
/// loaded with VioSetFont when that has cy rows.
pub fn query_font(c: &mut Ctx) -> u32 {
    let (sid, cy, p, cb) = (c.arg(0), c.arg(1) as usize, c.arg(2), c.arg(3));
    // Session 0: the caller's own (a DOS session's ROM fonts).
    let sid = if sid == 0 { c.process.session.load(Ordering::Relaxed) } else { sid };
    if !matches!(cy, 8 | 14 | 16) || (cb as usize) < 256 * cy {
        return err::INVALID_PARAMETER;
    }
    let Some(font) = crate::screen::with_session(sid, |s| {
        if s.user_font && s.cell_h == cy { s.font() } else { crate::screen::rom_font(s.codepage(), cy) }
    }) else {
        return ERROR_SMG_SESSION_NOT_FOUND;
    };
    if c.write(p, &font) { 0 } else { err::INVALID_ADDRESS }
}

/// Rs2VioKey(sid, pKey): a key for a windowable session, as an INPUTEVENT
/// of Rs2ReadInput (a = char | scan << 8 | status << 16 | key-up << 24,
/// flags = shift state, time).
pub fn key(c: &mut Ctx) -> u32 {
    let (sid, p) = (c.arg(0), c.arg(1));
    let Some(raw) = c.read_vec(p, 16) else { return err::INVALID_ADDRESS };
    if !session::is_windowable(sid) {
        return ERROR_SMG_SESSION_NOT_FOUND;
    }
    let u32_at = |o: usize| u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]);
    let a = u32_at(4);
    let k = crate::kbd::KeyInfo {
        ch: a as u8,
        scan: (a >> 8) as u8,
        status: (a >> 16) as u8,
        shift: u16::from_le_bytes([raw[2], raw[3]]),
        time: match u32_at(12) {
            0 => crate::irq::uptime_ms() as u32,
            t => t,
        },
    };
    crate::conkbd::inject_key(sid, k, a >> 24 & 1 != 0);
    0
}

/// Rs2VioMouse(sid, row, col, fsButtons): the pointer over a cell of a
/// windowable session's window (buttons: bit 0 left, 1 right, 2 middle).
pub fn mouse(c: &mut Ctx) -> u32 {
    let (sid, row, col, b) = (c.arg(0), c.arg(1) as i32, c.arg(2) as i32, c.arg(3));
    if !session::is_windowable(sid) {
        return ERROR_SMG_SESSION_NOT_FOUND;
    }
    crate::mouse::inject(sid, b as u8, row, col);
    0
}

/// Rs2VioControl(sid, cmd, arg):
/// 1 focus (arg: the window has it or not), 2 windowed (arg 1) or full
/// screen (arg 0), 3 end the session (its processes are killed),
/// 4 select any session (a windowed one's window is activated),
/// 5 register the caller as the host.
pub fn control(c: &mut Ctx) -> u32 {
    let (sid, cmd, arg) = (c.arg(0), c.arg(1), c.arg(2));
    match cmd {
        CTL_REGISTER => {
            viowin::register_host(c.process.pid);
            0
        }
        CTL_FOCUS => {
            viowin::set_focus(sid, arg != 0);
            0
        }
        CTL_WINDOWED => {
            if session::set_windowed(sid, arg != 0) { 0 } else { ERROR_SMG_SESSION_NOT_FOUND }
        }
        CTL_SELECT => {
            if session::select(sid) { 0 } else { ERROR_SMG_SESSION_NOT_FOUND }
        }
        CTL_STOP => {
            if sid == 1 || !session::exists(sid) || sid == session::pm_session() {
                return ERROR_SMG_SESSION_NOT_FOUND;
            }
            for p in crate::task::process::all() {
                if p.session.load(Ordering::Relaxed) == sid && p.exit_info().is_none() {
                    p.exit(3, 0); // TC_KILLPROCESS
                }
            }
            0
        }
        _ => err::INVALID_PARAMETER,
    }
}
