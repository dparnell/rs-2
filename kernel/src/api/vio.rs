//! VIOCALLS: the character-mode video API.
//!
//! VIO calls draw on the screen of the caller's session (or of the pop-up
//! its process owns). Every entry is 16-bit `_Pascal`; 32-bit programs
//! reach them through thunks. The last argument, the VIO handle, is 0
//! outside Presentation Manager AVIO and is not checked.
//!
//! * The logical video buffer (VioGetBuf) is a per-process copy of the
//!   screen that VioShowBuf transfers to it.
//! * The physical buffer (VioGetPhysBuf) is a shared object with the
//!   session's cells in VGA layout; what programs store in it is shown
//!   within a few ticks (screen::tick).
//! * VioSavRedrawWait / VioModeWait threads are told about session switches
//!   and pop-ups.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::screen;
use crate::sync::SpinLock;
use crate::task::WaitQueue;
use crate::vm::{self, pag, SegKind};

const ERROR_VIO_MODE: u32 = 355;
const ERROR_VIO_UNLOCK: u32 = 367;
const ERROR_VIO_FUNCTION_OWNED: u32 = 422;
const ERROR_VIO_RETURN: u32 = 423;
const ERROR_VIO_ILLEGAL_DURING_POPUP: u32 = 430;
const ERROR_VIO_FONT: u32 = 467;
const ERROR_VIO_TRANSPARENT_POPUP: u32 = 483;

fn rows_cols() -> (usize, usize) {
    screen::with(|s| (s.rows, s.cols)).unwrap_or((25, 80))
}

fn sid() -> u32 {
    crate::session::io_session()
}

// ---- Per-session VIO state ----

/// A thread waiting in VioSavRedrawWait or VioModeWait.
struct Waiter {
    pid: u32,
    /// VSRWI_REDRAW: only redraw notifications are wanted.
    redraw_only: bool,
    events: VecDeque<u16>,
    /// VioSavRedrawUndo / VioModeUndo cancelled the wait.
    cancelled: bool,
}

#[derive(Default)]
struct VioSess {
    /// Process holding VioScrLock.
    lock: Option<u32>,
    savredraw: Option<Waiter>,
    mode: Option<Waiter>,
    /// VioPrtScToggle: copy TTY output to the printer.
    prtsc_echo: bool,
}

static SESS: SpinLock<BTreeMap<u32, VioSess>> = SpinLock::new(BTreeMap::new());
static WAIT: WaitQueue = WaitQueue::new();

fn with_sess<R>(sid: u32, f: impl FnOnce(&mut VioSess) -> R) -> R {
    let mut g = SESS.lock();
    f(g.entry(sid).or_default())
}

fn post(w: &mut Option<Waiter>, ev: u16) {
    if let Some(w) = w {
        if !(w.redraw_only && ev == 0) {
            w.events.push_back(ev);
        }
    }
}

/// The foreground changed from session `old` to `new`: save and redraw
/// notifications (VSRWN_SAVE = 0, VSRWN_REDRAW = 1).
pub fn notify_switch(old: u32, new: u32) {
    {
        let mut g = SESS.lock();
        if let Some(s) = g.get_mut(&old) {
            post(&mut s.savredraw, 0);
        }
        if let Some(s) = g.get_mut(&new) {
            post(&mut s.savredraw, 1);
        }
    }
    WAIT.wake_all();
}

/// A pop-up appears over (`start`) or leaves session `fg`.
pub fn notify_popup(fg: u32, start: bool) {
    {
        let mut g = SESS.lock();
        if let Some(s) = g.get_mut(&fg) {
            post(&mut s.savredraw, if start { 0 } else { 1 });
            if !start {
                // VMWN_POPUP: restore the mode.
                post(&mut s.mode, 0);
            }
        }
    }
    WAIT.wake_all();
}

/// A process ended: its locks and waits go.
pub fn process_ended(pid: u32) {
    for s in SESS.lock().values_mut() {
        if s.lock == Some(pid) {
            s.lock = None;
        }
        if s.savredraw.as_ref().is_some_and(|w| w.pid == pid) {
            s.savredraw = None;
        }
        if s.mode.as_ref().is_some_and(|w| w.pid == pid) {
            s.mode = None;
        }
    }
    FONTBUF.lock().remove(&pid);
    WAIT.wake_all();
    crate::conkbd::process_ended(pid);
}

pub fn drop_session(sid: u32) {
    SESS.lock().remove(&sid);
}

// ---- Output ----

pub fn vio_wrt_tty(c: &mut Ctx) -> u32 {
    let (p, n) = (c.ptr(0), c.arg(1));
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    crate::console::write(&data);
    if with_sess(sid(), |s| s.prtsc_echo) {
        crate::chardev::lpt_write(0, &data);
    }
    0
}

pub fn vio_get_cur_pos(c: &mut Ctx) -> u32 {
    let (r, col) = screen::with(|s| (s.cur_row, s.cur_col.min(s.cols - 1))).unwrap_or((0, 0));
    c.write_u16(c.ptr(0), r as u16);
    c.write_u16(c.ptr(1), col as u16);
    0
}

pub fn vio_set_cur_pos(c: &mut Ctx) -> u32 {
    let (r, col) = (c.arg(0) as usize, c.arg(1) as usize);
    let (rows, cols) = rows_cols();
    if r >= rows {
        return err::VIO_ROW;
    }
    if col >= cols {
        return err::VIO_COL;
    }
    screen::with(|s| {
        s.cur_row = r;
        s.cur_col = col;
        s.set_cursor(Some((r, col)));
    });
    0
}

pub fn vio_get_cur_type(c: &mut Ctx) -> u32 {
    let shape = screen::with(|s| s.cursor_type).unwrap_or([14, 15, 1, 0]);
    let mut b = Vec::new();
    for w in shape {
        b.extend_from_slice(&w.to_le_bytes());
    }
    if c.write(c.ptr(0), &b) { 0 } else { err::INVALID_ADDRESS }
}

/// VioSetCurType(VIOCURSORINFO): yStart and cEnd are scan lines, or
/// percentages of the cell height when negative; attr -1 hides it.
pub fn vio_set_cur_type(c: &mut Ctx) -> u32 {
    let Some(raw) = c.read_vec(c.ptr(0), 8) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
    let t = [w(0), w(1), w(2), w(3)];
    let h = screen::with(|s| s.cell_h).unwrap_or(16) as i32;
    let ok = |v: u16| {
        let v = v as i16 as i32;
        v >= -100 && v < h.max(32)
    };
    if t[3] != 0xffff && (!ok(t[0]) || !ok(t[1]) || t[2] > 1) {
        return err::VIO_INVALID_PARMS;
    }
    screen::with(|s| {
        // Hiding keeps the shape; showing again restores it.
        let mut t = t;
        if t[3] == 0xffff {
            t[0] = s.cursor_type[0];
            t[1] = s.cursor_type[1];
            t[2] = s.cursor_type[2];
        }
        s.set_cursor_type(t);
        let pos = (s.cur_row, s.cur_col.min(s.cols - 1));
        s.set_cursor(Some(pos));
    });
    0
}

/// Fill VIOMODEINFO for the current text mode.
fn mode_bytes() -> Vec<u8> {
    let (rows, cols, h) = screen::with(|s| (s.rows, s.cols, s.cell_h)).unwrap_or((25, 80, 16));
    let mut b = Vec::new();
    b.extend_from_slice(&34u16.to_le_bytes()); // cb
    b.push(1); // fbType: VGMT_OTHER (colour), text
    b.push(4); // color: COLORS_16
    b.extend_from_slice(&(cols as u16).to_le_bytes());
    b.extend_from_slice(&(rows as u16).to_le_bytes());
    b.extend_from_slice(&((cols * screen::CELL_W) as u16).to_le_bytes()); // hres
    b.extend_from_slice(&((rows * h) as u16).to_le_bytes()); // vres
    b.push(0); // fmt_ID
    b.push(1); // attrib
    b.extend_from_slice(&0xb8000u32.to_le_bytes()); // buf_addr
    let len = (rows * cols * 2) as u32;
    b.extend_from_slice(&len.to_le_bytes()); // buf_length
    b.extend_from_slice(&len.to_le_bytes()); // full_length
    b.extend_from_slice(&len.to_le_bytes()); // partial_length
    b.extend_from_slice(&0u32.to_le_bytes()); // ext_data_addr
    b
}

pub fn vio_get_mode(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let Some(cb) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    if cb < 3 {
        return err::VIO_INVALID_LENGTH;
    }
    let b = mode_bytes();
    let n = (cb as usize).min(b.len());
    c.write(p + 2, &b[2..n]);
    0
}

/// VioSetMode(VIOMODEINFO): text modes of 40, 80 or 132 columns and any
/// number of rows a font fits (cb 3: type and colours only, 80x25). The
/// screen is cleared.
pub fn vio_set_mode(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let Some(cb) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    if cb < 3 {
        return err::VIO_INVALID_LENGTH;
    }
    let Some(raw) = c.read_vec(p, (cb as u32).min(12)) else { return err::INVALID_ADDRESS };
    let w = |i: usize| raw.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    if raw[2] & 2 != 0 {
        return ERROR_VIO_MODE; // graphics modes
    }
    if cb >= 4 && !matches!(raw[3], 0 | 1 | 2 | 4) {
        return ERROR_VIO_MODE;
    }
    let cols = if cb >= 6 { w(4).unwrap_or(80) } else { 80 };
    let rows = if cb >= 8 { w(6).unwrap_or(25) } else { 25 };
    if !(cols == 40 || cols == 80 || cols == 132) {
        return ERROR_VIO_MODE;
    }
    let fits = screen::with(|s| s.mode_fits(cols, rows)).unwrap_or(false);
    if !fits {
        return ERROR_VIO_MODE;
    }
    screen::with(|s| {
        s.set_mode(cols, rows);
        for r in 0..s.rows {
            for col in 0..s.cols {
                s.put(r, col, 0x0720);
            }
        }
        s.cur_row = 0;
        s.cur_col = 0;
        s.set_cursor(Some((0, 0)));
    });
    0
}

/// Read a (char, attr) fill cell from user memory.
fn fill_cell(c: &Ctx, p: u32) -> Option<u16> {
    let b = c.read_vec(p, 2)?;
    Some(b[0] as u16 | (b[1] as u16) << 8)
}

/// VioScrollUp/Dn/Lf/Rt(top, left, bottom, right, lines, pcell, hvio).
/// Bottom and right are clipped to the screen; -1 lines clears the area.
fn scroll(c: &mut Ctx, dir: u8) -> u32 {
    let (top, left, bottom, right, n) = (c.arg(0) as usize, c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize, c.arg(4) as usize);
    let Some(fill) = fill_cell(c, c.ptr(5)) else { return err::INVALID_ADDRESS };
    let (rows, cols) = rows_cols();
    if top >= rows {
        return err::VIO_ROW;
    }
    if left >= cols {
        return err::VIO_COL;
    }
    screen::with(|s| {
        let bottom = bottom.min(s.rows - 1);
        let right = right.min(s.cols - 1);
        if top > bottom || left > right || n == 0 {
            return;
        }
        let h = bottom - top + 1;
        let w = right - left + 1;
        let n = n.min(if dir < 2 { h } else { w });
        // Whole-width scroll up: move pixels instead of redrawing.
        if dir == 0 && left == 0 && right == s.cols - 1 {
            s.scroll_up(top, bottom, n, fill);
            return;
        }
        let mut buf: Vec<u16> = Vec::with_capacity(h * w);
        for r in top..=bottom {
            for col in left..=right {
                buf.push(s.cell(r, col));
            }
        }
        let get = |r: isize, col: isize| -> u16 {
            if r < 0 || col < 0 || r as usize >= h || col as usize >= w {
                fill
            } else {
                buf[r as usize * w + col as usize]
            }
        };
        for r in 0..h {
            for col in 0..w {
                let (sr, sc) = match dir {
                    0 => (r as isize + n as isize, col as isize), // up
                    1 => (r as isize - n as isize, col as isize), // down
                    2 => (r as isize, col as isize + n as isize), // left
                    _ => (r as isize, col as isize - n as isize), // right
                };
                let v = get(sr, sc);
                if v != s.cell(top + r, left + col) {
                    s.put(top + r, left + col, v);
                }
            }
        }
    });
    0
}

pub fn vio_scroll_up(c: &mut Ctx) -> u32 {
    scroll(c, 0)
}

pub fn vio_scroll_dn(c: &mut Ctx) -> u32 {
    scroll(c, 1)
}

pub fn vio_scroll_lf(c: &mut Ctx) -> u32 {
    scroll(c, 2)
}

pub fn vio_scroll_rt(c: &mut Ctx) -> u32 {
    scroll(c, 3)
}

fn check_pos(row: usize, col: usize) -> Result<(), u32> {
    let (rows, cols) = rows_cols();
    if row >= rows {
        return Err(err::VIO_ROW);
    }
    if col >= cols {
        return Err(err::VIO_COL);
    }
    Ok(())
}

/// Apply `f` to `n` cells from (row, col), wrapping at the line end and
/// stopping at the end of the screen.
fn modify_run(n: usize, row: usize, col: usize, mut f: impl FnMut(usize, u16) -> u16) -> u32 {
    if let Err(e) = check_pos(row, col) {
        return e;
    }
    screen::with(|s| {
        let (mut r, mut cc) = (row, col);
        for i in 0..n {
            if r >= s.rows {
                break;
            }
            let v = f(i, s.cell(r, cc));
            s.put(r, cc, v);
            cc += 1;
            if cc >= s.cols {
                cc = 0;
                r += 1;
            }
        }
    });
    0
}

pub fn vio_wrt_char_str(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    // Characters keep the existing attributes.
    modify_run(data.len(), row, col, |i, old| (old & 0xff00) | data[i] as u16)
}

pub fn vio_wrt_char_str_att(c: &mut Ctx) -> u32 {
    let (p, n, row, col, pa) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize, c.ptr(4));
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    let Some(attr) = c.read_u8(pa) else { return err::INVALID_ADDRESS };
    modify_run(data.len(), row, col, |i, _| data[i] as u16 | (attr as u16) << 8)
}

pub fn vio_wrt_cell_str(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some(data) = c.read_vec(p, n & !1) else { return err::INVALID_ADDRESS };
    modify_run(data.len() / 2, row, col, |i, _| data[i * 2] as u16 | (data[i * 2 + 1] as u16) << 8)
}

pub fn vio_wrt_n_cell(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(cell) = fill_cell(c, p) else { return err::INVALID_ADDRESS };
    modify_run(n, row, col, |_, _| cell)
}

pub fn vio_wrt_n_char(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(ch) = c.read_u8(p) else { return err::INVALID_ADDRESS };
    modify_run(n, row, col, |_, old| (old & 0xff00) | ch as u16)
}

pub fn vio_wrt_n_attr(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(a) = c.read_u8(p) else { return err::INVALID_ADDRESS };
    modify_run(n, row, col, |_, old| (old & 0x00ff) | (a as u16) << 8)
}

/// VioReadCellStr / VioReadCharStr(pch, pcb, row, col, hvio): *pcb is the
/// buffer size in, the bytes read out.
fn read_run(c: &mut Ctx, cells: bool) -> u32 {
    let (p, pcb, row, col) = (c.ptr(0), c.ptr(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some(cb) = c.read_u16(pcb) else { return err::INVALID_ADDRESS };
    if let Err(e) = check_pos(row, col) {
        return e;
    }
    let per = if cells { 2 } else { 1 };
    let n = cb as usize / per;
    let v = screen::with(|s| {
        let mut v = Vec::with_capacity(cb as usize);
        let (mut r, mut cc) = (row, col);
        for _ in 0..n {
            if r >= s.rows {
                break;
            }
            let cell = s.cell(r, cc);
            v.push(cell as u8);
            if cells {
                v.push((cell >> 8) as u8);
            }
            cc += 1;
            if cc >= s.cols {
                cc = 0;
                r += 1;
            }
        }
        v
    })
    .unwrap_or_default();
    c.write(p, &v);
    c.write_u16(pcb, v.len() as u16);
    0
}

pub fn vio_read_cell_str(c: &mut Ctx) -> u32 {
    read_run(c, true)
}

pub fn vio_read_char_str(c: &mut Ctx) -> u32 {
    read_run(c, false)
}

/// VioGetConfig(usConfigId, pvioin, hvio): a VGA with a colour display.
pub fn vio_get_config(c: &mut Ctx) -> u32 {
    let (id, p) = (c.arg(0), c.ptr(1));
    if id > 1 {
        return err::VIO_INVALID_PARMS; // no secondary adapter
    }
    let Some(cb) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    if cb < 2 {
        return err::VIO_INVALID_LENGTH;
    }
    let mut b = Vec::new();
    b.extend_from_slice(&3u16.to_le_bytes()); // adapter: DISPLAY_VGA
    b.extend_from_slice(&4u16.to_le_bytes()); // display: MONITOR_851X_COLOR
    b.extend_from_slice(&(256 * 1024u32).to_le_bytes()); // cbMemory
    b.extend_from_slice(&1u16.to_le_bytes()); // Configuration
    b.extend_from_slice(&0x0100u16.to_le_bytes()); // VDHVersion
    b.extend_from_slice(&0u16.to_le_bytes()); // Flags
    b.extend_from_slice(&0x8000u32.to_le_bytes()); // HWBufferSize
    b.extend_from_slice(&0x8000u32.to_le_bytes()); // FullSaveSize
    b.extend_from_slice(&0x1000u32.to_le_bytes()); // PartSaveSize
    b.extend_from_slice(&0u16.to_le_bytes()); // EMAdaptersOFF
    b.extend_from_slice(&0u16.to_le_bytes()); // EMDisplaysOFF
    let n = (cb as usize).saturating_sub(2).min(b.len());
    c.write(p + 2, &b[..n]);
    0
}

pub fn vio_get_cp(c: &mut Ctx) -> u32 {
    let cp = screen::with(|s| s.codepage().number()).unwrap_or(850);
    c.write_u16(c.ptr(1), cp);
    0
}

pub fn vio_set_cp(c: &mut Ctx) -> u32 {
    let cp = c.arg(1) as u16;
    let Some(cp) = crate::codepage::CodePage::from_number(cp) else { return 355 }; // ERROR_VIO_BAD_CP
    screen::with(|s| s.set_codepage(cp));
    0
}

pub fn vio_get_ansi(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), crate::console::ansi_enabled() as u16);
    0
}

pub fn vio_set_ansi(c: &mut Ctx) -> u32 {
    match c.arg(0) {
        0 => crate::console::set_ansi(false),
        1 => crate::console::set_ansi(true),
        _ => return err::VIO_INVALID_PARMS,
    }
    0
}

// ---- VioGetState / VioSetState ----

/// VioGetState(pState, hvio): the request block starts with cb and type.
///
/// | type | block |
/// |------|-------|
/// | 0 | VIOPALSTATE: iFirst, acolor[] (palette registers) |
/// | 1 | VIOOVERSCAN: border colour |
/// | 2 | VIOINTENSITY: 0 blink, 1 background intensity |
/// | 3 | VIOCOLORREG: first, count, far pointer to RGB triples (DAC) |
/// | 4 | VIOSETULINELOC: underline scan line |
/// | 5 | VIOSETTARGET: display VioSetMode applies to |
pub fn vio_get_state(c: &mut Ctx) -> u32 {
    state(c, false)
}

pub fn vio_set_state(c: &mut Ctx) -> u32 {
    state(c, true)
}

static TARGET: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

fn state(c: &mut Ctx, set: bool) -> u32 {
    let p = c.ptr(0);
    let Some(hdr) = c.read_vec(p, 4) else { return err::INVALID_ADDRESS };
    let cb = u16::from_le_bytes([hdr[0], hdr[1]]) as u32;
    let ty = u16::from_le_bytes([hdr[2], hdr[3]]);
    let need = match ty {
        0 => 8,
        1 | 2 | 4 | 5 => 6,
        3 => 12,
        _ => return err::VIO_INVALID_PARMS,
    };
    if cb < need {
        return err::VIO_INVALID_LENGTH;
    }
    let Some(body) = c.read_vec(p + 4, cb - 4) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([body[i * 2], body[i * 2 + 1]]);
    match ty {
        0 => {
            let first = w(0) as usize;
            let n = ((cb - 6) / 2) as usize;
            if first + n > 16 {
                return err::VIO_INVALID_PARMS;
            }
            if set {
                screen::with(|s| {
                    for i in 0..n {
                        s.palregs[first + i] = w(1 + i) as u8 & 0x3f;
                    }
                    s.update_palette();
                    s.redraw();
                });
            } else {
                let regs = screen::with(|s| s.palregs).unwrap_or_default();
                for i in 0..n {
                    c.write_u16(p + 6 + i as u32 * 2, regs[first + i] as u16);
                }
            }
        }
        1 => {
            if set {
                screen::with(|s| s.set_overscan(w(0) as u8));
            } else {
                c.write_u16(p + 4, screen::with(|s| s.overscan).unwrap_or(0) as u16);
            }
        }
        2 => {
            if set {
                screen::with(|s| s.set_blink(w(0) == 0));
            } else {
                c.write_u16(p + 4, screen::with(|s| !s.blink).unwrap_or(false) as u16);
            }
        }
        3 => {
            let (first, n) = (w(0) as usize, w(1) as usize);
            let at = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let at = c.far16_to_flat(at);
            if first + n > 256 {
                return err::VIO_INVALID_PARMS;
            }
            if set {
                let Some(rgb) = c.read_vec(at, n as u32 * 3) else { return err::INVALID_ADDRESS };
                screen::with(|s| {
                    for i in 0..n {
                        s.dac[first + i] = (rgb[i * 3] & 0x3f, rgb[i * 3 + 1] & 0x3f, rgb[i * 3 + 2] & 0x3f);
                    }
                    s.update_palette();
                    s.redraw();
                });
            } else {
                let dac = screen::with(|s| s.dac).unwrap_or([(0, 0, 0); 256]);
                let mut rgb = Vec::with_capacity(n * 3);
                for &(r, g, b) in &dac[first..first + n] {
                    rgb.extend_from_slice(&[r, g, b]);
                }
                if !c.write(at, &rgb) {
                    return err::INVALID_ADDRESS;
                }
            }
        }
        4 => {
            if set {
                let h = screen::with(|s| s.cell_h).unwrap_or(16) as u16;
                if w(0) >= 32 && w(0) != 0xffff {
                    return err::VIO_INVALID_PARMS;
                }
                let _ = h;
                screen::with(|s| s.underline = w(0));
            } else {
                c.write_u16(p + 4, screen::with(|s| s.underline).unwrap_or(31));
            }
        }
        _ => {
            if set {
                if w(0) > 2 {
                    return err::VIO_INVALID_PARMS;
                }
                TARGET.store(w(0), core::sync::atomic::Ordering::Relaxed);
            } else {
                c.write_u16(p + 4, TARGET.load(core::sync::atomic::Ordering::Relaxed));
            }
        }
    }
    0
}

// ---- Fonts ----

/// Buffers handed out by VioGetFont with a null pbData, by process.
static FONTBUF: SpinLock<BTreeMap<u32, u32>> = SpinLock::new(BTreeMap::new());

/// VioGetFont(VIOFONTINFO {cb, type, cxCell, cyCell, pbData, cbData}):
/// type 0 is the font in use, type 1 the built-in font of the given cell
/// size (8x8, 8x14, 8x16). A null pbData gets a system buffer.
pub fn vio_get_font(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let Some(raw) = c.read_vec(p, 14) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([raw[i], raw[i + 1]]);
    if w(0) < 14 {
        return err::VIO_INVALID_LENGTH;
    }
    let (ty, cx, cy, pdata, cbdata) = (w(2), w(4), w(6), u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]), w(12));
    let (font, h) = match ty {
        0 => match screen::with(|s| (s.font(), s.cell_h)) {
            Some(x) => x,
            None => return ERROR_VIO_FONT,
        },
        1 => {
            if cx != 8 || !screen::FONT_HEIGHTS.contains(&(cy as usize)) {
                return ERROR_VIO_FONT;
            }
            let cp = screen::with(|s| s.codepage()).unwrap_or(crate::codepage::CodePage::Cp850);
            (screen::rom_font(cp, cy as usize), cy as usize)
        }
        _ => return err::VIO_INVALID_PARMS,
    };
    let mut at = pdata;
    if pdata == 0 {
        let pid = c.process.pid;
        let have = FONTBUF.lock().get(&pid).copied();
        let base = match have {
            Some(b) => b,
            None => {
                let r = c.space().alloc(256 * 16, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None);
                match r {
                    Ok(b) => {
                        FONTBUF.lock().insert(pid, b);
                        b
                    }
                    Err(e) => return e.code(),
                }
            }
        };
        at = (vm::tile_selector(base) as u32) << 16;
    } else if (cbdata as usize) < font.len() {
        return err::VIO_INVALID_LENGTH;
    }
    let flat = c.far16_to_flat(at);
    if !c.write(flat, &font) {
        return err::INVALID_ADDRESS;
    }
    c.write_u16(p + 4, 8);
    c.write_u16(p + 6, h as u16);
    c.write_u32(p + 8, at);
    c.write_u16(p + 12, font.len() as u16);
    0
}

/// VioSetFont(VIOFONTINFO): load an 8-pixel wide font of cyCell rows for
/// the current mode.
pub fn vio_set_font(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let Some(raw) = c.read_vec(p, 14) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([raw[i], raw[i + 1]]);
    if w(0) < 14 {
        return err::VIO_INVALID_LENGTH;
    }
    let (ty, cx, cy, pdata, cbdata) = (w(2), w(4), w(6) as usize, u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]), w(12) as usize);
    if ty != 0 || cx != 8 || !(1..=screen::CELL_H).contains(&cy) || cbdata < 256 * cy {
        return ERROR_VIO_FONT;
    }
    let flat = c.far16_to_flat(pdata);
    let Some(data) = c.read_vec(flat, (256 * cy) as u32) else { return err::INVALID_ADDRESS };
    if screen::with(|s| s.set_font(cy, &data)).unwrap_or(false) { 0 } else { ERROR_VIO_FONT }
}

// ---- Buffers ----

pub fn vio_get_buf(c: &mut Ctx) -> u32 {
    let (rows, cols) = rows_cols();
    let len = (rows * cols * 2) as u32;
    let base = match lvb(c) {
        Ok(b) => b,
        Err(e) => return e,
    };
    // Prime the buffer with the current screen contents.
    let cells: Vec<u8> = screen::with(|s| {
        let mut v = Vec::with_capacity(len as usize);
        for r in 0..s.rows {
            for col in 0..s.cols {
                let cell = s.cell(r, col);
                v.push(cell as u8);
                v.push((cell >> 8) as u8);
            }
        }
        v
    })
    .unwrap_or_default();
    c.write(base, &cells);
    c.write_u32(c.ptr(0), (vm::tile_selector(base) as u32) << 16);
    c.write_u16(c.ptr(1), len as u16);
    0
}

/// The process's logical video buffer (allocated on first use, big enough
/// for every mode).
fn lvb(c: &Ctx) -> Result<u32, u32> {
    let mut slot = c.process.lvb.lock();
    if *slot != 0 {
        return Ok(*slot);
    }
    let size = (screen::MAX_ROWS * screen::MAX_COLS * 2) as u32;
    let r = c.space().alloc(size.next_multiple_of(0x1000), pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None);
    let base = r.map_err(|e| e.code())?;
    *slot = base;
    Ok(base)
}

pub fn vio_show_buf(c: &mut Ctx) -> u32 {
    let (off, len) = (c.arg(0), c.arg(1));
    let base = *c.process.lvb.lock();
    if base == 0 {
        return err::VIO_INVALID_PARMS;
    }
    let (rows, cols) = rows_cols();
    let total = (rows * cols * 2) as u32;
    if off >= total {
        return err::VIO_INVALID_PARMS;
    }
    let len = len.min(total - (off & !1));
    let Some(data) = c.read_vec(base + (off & !1), len.max(2) & !1) else { return err::INVALID_ADDRESS };
    screen::with(|s| {
        let cols = s.cols;
        for (i, ch) in data.chunks(2).enumerate() {
            let pos = (off as usize / 2) + i;
            let (r, col) = (pos / cols, pos % cols);
            if r >= s.rows {
                break;
            }
            let v = ch[0] as u16 | (ch[1] as u16) << 8;
            if s.cell(r, col) != v {
                s.put(r, col, v);
            }
        }
    });
    0
}

/// VioGetPhysBuf(VIOPHYSBUF {pBuf, cb, asel[]}, usReserved): selectors for
/// the "video memory" at pBuf (B8000 or B0000): the session's physical
/// text buffer, shown on the screen.
pub fn vio_get_phys_buf(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let (Some(addr), Some(cb)) = (c.read_u32(p), c.read_u32(p + 4)) else { return err::INVALID_ADDRESS };
    if !(0xa0000..0xc0000).contains(&addr) || cb == 0 || addr as u64 + cb as u64 > 0xc0000 {
        return err::VIO_INVALID_PARMS;
    }
    let s = sid();
    let Some(obj) = screen::with_session(s, |sc| sc.phys_buf()).flatten() else { return err::NOT_ENOUGH_MEMORY };
    {
        let mut sp = c.space();
        if !sp.regions.contains_key(&obj.base) {
            sp.map_shared(obj.clone(), pag::READ | pag::WRITE, SegKind::Data);
        }
    }
    // The buffer starts at B8000 (colour text); other addresses below it
    // see the same cells.
    let off = addr.saturating_sub(0xb8000) % screen::PhysBuf::SIZE;
    let n = (off + cb).div_ceil(0x10000).max(1);
    for i in 0..n {
        c.write_u16(p + 8 + i * 2, vm::tile_selector(obj.base + i * 0x10000));
    }
    0
}

// ---- Screen lock ----

/// VioScrLock(fWait, pfNotLocked, hvio).
pub fn vio_scr_lock(c: &mut Ctx) -> u32 {
    let (wait, pnot) = (c.arg(0), c.ptr(1));
    if wait > 1 {
        return 366; // ERROR_VIO_WAIT_FLAG
    }
    let (s, pid) = (sid(), c.process.pid);
    loop {
        let got = with_sess(s, |v| match v.lock {
            None => {
                v.lock = Some(pid);
                true
            }
            Some(o) => o == pid,
        });
        // A background session cannot get the screen without waiting.
        if got {
            c.write_u8(pnot, 0);
            return 0;
        }
        if wait == 0 {
            c.write_u8(pnot, 1);
            return 0;
        }
        if !WAIT.wait_until(None, || with_sess(s, |v| v.lock.is_none())) {
            return err::INTERRUPT;
        }
    }
}

pub fn vio_scr_unlock(c: &mut Ctx) -> u32 {
    let (s, pid) = (sid(), c.process.pid);
    let ok = with_sess(s, |v| {
        if v.lock == Some(pid) {
            v.lock = None;
            true
        } else {
            false
        }
    });
    WAIT.wake_all();
    if ok { 0 } else { ERROR_VIO_UNLOCK }
}

// ---- Save/redraw and mode notifications ----

fn pick(v: &mut VioSess, mode: bool) -> &mut Option<Waiter> {
    if mode { &mut v.mode } else { &mut v.savredraw }
}

/// Wait for a notification in `slot` of the caller's session.
fn wait_notify(c: &mut Ctx, mode: bool, redraw_only: bool, pnotify: u32) -> u32 {
    let (s, pid) = (crate::session::current(), c.process.pid);
    let owned = with_sess(s, |v| match pick(v, mode) {
        Some(w) if w.pid != pid => true,
        Some(w) => {
            w.redraw_only = redraw_only;
            w.cancelled = false;
            false
        }
        slot @ None => {
            *slot = Some(Waiter { pid, redraw_only, events: VecDeque::new(), cancelled: false });
            false
        }
    });
    if owned {
        return ERROR_VIO_FUNCTION_OWNED;
    }
    loop {
        let r = with_sess(s, |v| match pick(v, mode) {
            None => Some(Err(ERROR_VIO_RETURN)),
            Some(w) if w.cancelled => {
                w.cancelled = false;
                Some(Err(ERROR_VIO_RETURN))
            }
            Some(w) => w.events.pop_front().map(Ok),
        });
        match r {
            Some(Ok(ev)) => {
                c.write_u16(pnotify, ev);
                return 0;
            }
            Some(Err(e)) => return e,
            None => {}
        }
        let ready = || SESS.lock().get(&s).and_then(|v| if mode { v.mode.as_ref() } else { v.savredraw.as_ref() }).is_none_or(|w| w.cancelled || !w.events.is_empty());
        if !WAIT.wait_until(None, ready) {
            return err::INTERRUPT;
        }
    }
}

/// VioSavRedrawUndo / VioModeUndo(ownerInd, killInd, reserved).
fn undo(c: &mut Ctx, mode: bool) -> u32 {
    let (owner, kill) = (c.arg(0), c.arg(1));
    if owner > 1 || kill > 1 {
        return err::VIO_INVALID_PARMS;
    }
    let (s, pid) = (crate::session::current(), c.process.pid);
    let r = with_sess(s, |v| {
        let slot = if mode { &mut v.mode } else { &mut v.savredraw };
        match slot {
            Some(w) if w.pid == pid => {
                w.cancelled = true;
                if owner == 1 {
                    *slot = None;
                }
                0
            }
            Some(_) => ERROR_VIO_FUNCTION_OWNED,
            None => 0,
        }
    });
    WAIT.wake_all();
    r
}

/// VioSavRedrawWait(usRedrawInd, pNotifyType, reserved).
pub fn vio_sav_redraw_wait(c: &mut Ctx) -> u32 {
    let (ind, p) = (c.arg(0), c.ptr(1));
    if ind > 1 {
        return err::VIO_INVALID_PARMS;
    }
    wait_notify(c, false, ind == 1, p)
}

pub fn vio_sav_redraw_undo(c: &mut Ctx) -> u32 {
    undo(c, false)
}

/// VioModeWait(usReqType, pNotifyType, reserved): VMWR_POPUP only.
pub fn vio_mode_wait(c: &mut Ctx) -> u32 {
    let (ty, p) = (c.arg(0), c.ptr(1));
    if ty != 0 {
        return err::VIO_INVALID_PARMS;
    }
    wait_notify(c, true, false, p)
}

pub fn vio_mode_undo(c: &mut Ctx) -> u32 {
    undo(c, true)
}

// ---- Pop-ups ----

/// VioPopUp(pfWait, hvio): VP_WAIT (1), VP_TRANSPARENT (2).
pub fn vio_pop_up(c: &mut Ctx) -> u32 {
    let Some(f) = c.read_u16(c.ptr(0)) else { return err::INVALID_ADDRESS };
    if f & !3 != 0 {
        return err::VIO_INVALID_PARMS;
    }
    let transparent = f & 2 != 0;
    if transparent && crate::session::foreground() == crate::session::pm_session() && crate::session::pm_session() != 0 {
        return ERROR_VIO_TRANSPARENT_POPUP;
    }
    match crate::session::popup_begin(f & 1 != 0, transparent) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

pub fn vio_end_pop_up(_c: &mut Ctx) -> u32 {
    match crate::session::popup_end() {
        Ok(()) => 0,
        Err(e) => e,
    }
}

// ---- Printing ----

/// The text of session `sid`'s screen, one CR LF terminated line per row.
fn screen_text(sid: u32) -> Vec<u8> {
    screen::with_session(sid, |s| {
        let mut v = Vec::new();
        for r in 0..s.rows {
            let mut line: Vec<u8> = (0..s.cols).map(|c| s.cell(r, c) as u8).collect();
            while line.last() == Some(&b' ') || line.last() == Some(&0) {
                line.pop();
            }
            v.extend_from_slice(&line);
            v.extend_from_slice(b"\r\n");
        }
        v
    })
    .unwrap_or_default()
}

/// The PrtSc key and VioPrtSc: the screen goes to LPT1.
pub fn print_screen(sid: u32) {
    let text = screen_text(sid);
    crate::chardev::lpt_write(0, &text);
}

pub fn vio_prt_sc(_c: &mut Ctx) -> u32 {
    if crate::session::popup_active() && !crate::session::popup_owner(_c.process.pid) {
        return ERROR_VIO_ILLEGAL_DURING_POPUP;
    }
    print_screen(sid());
    0
}

/// VioPrtScToggle: echo TTY output to the printer.
pub fn vio_prt_sc_toggle(_c: &mut Ctx) -> u32 {
    with_sess(sid(), |s| s.prtsc_echo = !s.prtsc_echo);
    0
}

/// VioCheckCharType(pType, row, col, hvio): single-byte code pages only.
pub fn vio_check_char_type(c: &mut Ctx) -> u32 {
    let (row, col) = (c.arg(1) as usize, c.arg(2) as usize);
    if let Err(e) = check_pos(row, col) {
        return e;
    }
    c.write_u16(c.ptr(0), 0); // VCC_SBCSCHAR
    0
}

pub fn ok(_c: &mut Ctx) -> u32 {
    0
}
