//! VIOCALLS: the character-mode video API.
//!
//! One text session for now: VIO calls draw on the physical screen. The
//! logical video buffer handed out by VioGetBuf is a per-process copy that
//! VioShowBuf transfers to the screen.

use alloc::vec::Vec;

use super::{err, Ctx};
use crate::screen;
use crate::vm::{self, pag, SegKind};

fn cell_at(s: &screen::Screen, r: usize, c: usize) -> u16 {
    s.cell(r, c)
}

fn rows_cols() -> (usize, usize) {
    screen::with(|s| (s.rows, s.cols)).unwrap_or((25, 80))
}

pub fn vio_wrt_tty(c: &mut Ctx) -> u32 {
    let (p, n) = (c.ptr(0), c.arg(1));
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    crate::console::write(&data);
    0
}

pub fn vio_get_cur_pos(c: &mut Ctx) -> u32 {
    let (r, col) = screen::with(|s| (s.cur_row, s.cur_col)).unwrap_or((0, 0));
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
        if CURSOR_VISIBLE.load(core::sync::atomic::Ordering::Relaxed) {
            s.set_cursor(Some((r, col)));
        }
    });
    0
}

static CURSOR_VISIBLE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
static CURSOR_SHAPE: crate::sync::SpinLock<[u16; 4]> = crate::sync::SpinLock::new([14, 15, 1, 0]);

pub fn vio_get_cur_type(c: &mut Ctx) -> u32 {
    let shape = *CURSOR_SHAPE.lock();
    let mut b = Vec::new();
    for w in shape {
        b.extend_from_slice(&w.to_le_bytes());
    }
    if c.write(c.ptr(0), &b) { 0 } else { err::INVALID_ADDRESS }
}

pub fn vio_set_cur_type(c: &mut Ctx) -> u32 {
    let Some(raw) = c.read_vec(c.ptr(0), 8) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
    *CURSOR_SHAPE.lock() = [w(0), w(1), w(2), w(3)];
    let visible = w(3) != 0xffff;
    CURSOR_VISIBLE.store(visible, core::sync::atomic::Ordering::Relaxed);
    screen::with(|s| {
        let pos = (s.cur_row, s.cur_col);
        s.set_cursor(visible.then_some(pos));
    });
    0
}

/// Fill VIOMODEINFO for the current text mode.
fn mode_bytes() -> Vec<u8> {
    let (rows, cols) = rows_cols();
    let mut b = Vec::new();
    b.extend_from_slice(&34u16.to_le_bytes()); // cb
    b.push(1); // fbType: colour text
    b.push(4); // color: 16 colours
    b.extend_from_slice(&(cols as u16).to_le_bytes());
    b.extend_from_slice(&(rows as u16).to_le_bytes());
    b.extend_from_slice(&((cols * 9) as u16).to_le_bytes()); // hres
    b.extend_from_slice(&((rows * 16) as u16).to_le_bytes()); // vres
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
    if cb < 2 {
        return err::VIO_INVALID_LENGTH;
    }
    let b = mode_bytes();
    let n = (cb as usize).min(b.len());
    c.write(p + 2, &b[2..n]);
    0
}

pub fn vio_set_mode(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let Some(raw) = c.read_vec(p, 8) else { return err::INVALID_ADDRESS };
    let cb = u16::from_le_bytes([raw[0], raw[1]]);
    if cb >= 8 {
        let cols = u16::from_le_bytes([raw[4], raw[5]]) as usize;
        let rows = u16::from_le_bytes([raw[6], raw[7]]) as usize;
        if raw[2] & 2 != 0 {
            return err::VIO_INVALID_PARMS; // graphics modes
        }
        if !(cols == 40 || cols == 80 || cols == 132) || rows == 0 || rows > screen::MAX_ROWS {
            return err::VIO_INVALID_PARMS;
        }
        screen::with(|s| s.set_mode(cols, rows));
    }
    0
}

/// Read a (char, attr) fill cell from user memory.
fn fill_cell(c: &Ctx, p: u32) -> Option<u16> {
    let b = c.read_vec(p, 2)?;
    Some(b[0] as u16 | (b[1] as u16) << 8)
}

/// VioScrollUp/Dn/Lf/Rt(top, left, bottom, right, lines, pcell, hvio).
fn scroll(c: &mut Ctx, dir: u8) -> u32 {
    let (top, left, bottom, right, n) = (c.arg(0) as usize, c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize, c.arg(4) as usize);
    let Some(fill) = fill_cell(c, c.ptr(5)) else { return err::INVALID_ADDRESS };
    screen::with(|s| {
        let bottom = bottom.min(s.rows - 1);
        let right = right.min(s.cols - 1);
        if top > bottom || left > right {
            return;
        }
        let h = bottom - top + 1;
        let w = right - left + 1;
        let n = n.min(if dir < 2 { h } else { w });
        let mut buf: Vec<u16> = Vec::with_capacity(h * w);
        for r in top..=bottom {
            for col in left..=right {
                buf.push(cell_at(s, r, col));
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
                s.put(top + r, left + col, get(sr, sc));
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

/// Write `cells` starting at (row, col), wrapping at the line end and
/// stopping at the end of the screen.
fn put_run(cells: impl Iterator<Item = u16>, row: usize, col: usize) -> u32 {
    screen::with(|s| {
        if row >= s.rows || col >= s.cols {
            return err::VIO_ROW;
        }
        let (mut r, mut cc) = (row, col);
        for cell in cells {
            if r >= s.rows {
                break;
            }
            s.put(r, cc, cell);
            cc += 1;
            if cc >= s.cols {
                cc = 0;
                r += 1;
            }
        }
        0
    })
    .unwrap_or(0)
}

/// Apply `f` to existing cells in a run.
fn modify_run(n: usize, row: usize, col: usize, f: impl Fn(u16) -> u16) -> u32 {
    screen::with(|s| {
        if row >= s.rows || col >= s.cols {
            return err::VIO_ROW;
        }
        let (mut r, mut cc) = (row, col);
        for _ in 0..n {
            if r >= s.rows {
                break;
            }
            let v = f(s.cell(r, cc));
            s.put(r, cc, v);
            cc += 1;
            if cc >= s.cols {
                cc = 0;
                r += 1;
            }
        }
        0
    })
    .unwrap_or(0)
}

pub fn vio_wrt_char_str(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    // Characters keep the existing attributes.
    screen::with(|s| {
        let (mut r, mut cc) = (row, col);
        for &ch in &data {
            if r >= s.rows {
                break;
            }
            let old = s.cell(r, cc);
            s.put(r, cc, (old & 0xff00) | ch as u16);
            cc += 1;
            if cc >= s.cols {
                cc = 0;
                r += 1;
            }
        }
    });
    0
}

pub fn vio_wrt_char_str_att(c: &mut Ctx) -> u32 {
    let (p, n, row, col, pa) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize, c.ptr(4));
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    let Some(attr) = c.read_u8(pa) else { return err::INVALID_ADDRESS };
    put_run(data.iter().map(|&ch| ch as u16 | (attr as u16) << 8), row, col)
}

pub fn vio_wrt_cell_str(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some(data) = c.read_vec(p, n & !1) else { return err::INVALID_ADDRESS };
    put_run(data.chunks(2).map(|b| b[0] as u16 | (b[1] as u16) << 8), row, col)
}

pub fn vio_wrt_n_cell(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(cell) = fill_cell(c, p) else { return err::INVALID_ADDRESS };
    put_run(core::iter::repeat_n(cell, n), row, col)
}

pub fn vio_wrt_n_char(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(ch) = c.read_u8(p) else { return err::INVALID_ADDRESS };
    modify_run(n, row, col, |old| (old & 0xff00) | ch as u16)
}

pub fn vio_wrt_n_attr(c: &mut Ctx) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(a) = c.read_u8(p) else { return err::INVALID_ADDRESS };
    modify_run(n, row, col, |old| (old & 0x00ff) | (a as u16) << 8)
}

fn read_run(c: &mut Ctx, cells: bool) -> u32 {
    let (p, pcb, row, col) = (c.ptr(0), c.ptr(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some(cb) = c.read_u16(pcb) else { return err::INVALID_ADDRESS };
    let per = if cells { 2 } else { 1 };
    let n = cb as usize / per;
    let out = screen::with(|s| {
        let mut v = Vec::with_capacity(cb as usize);
        if row >= s.rows || col >= s.cols {
            return None;
        }
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
        Some(v)
    })
    .flatten();
    let Some(v) = out else { return err::VIO_ROW };
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

pub fn vio_get_config(c: &mut Ctx) -> u32 {
    // VioGetConfig(usConfigId, pvioin, hvio)
    let p = c.ptr(1);
    let Some(cb) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    let mut b = Vec::new();
    b.extend_from_slice(&3u16.to_le_bytes()); // adapter: VGA
    b.extend_from_slice(&3u16.to_le_bytes()); // display: VGA colour
    b.extend_from_slice(&(256 * 1024u32).to_le_bytes()); // cbMemory
    b.extend_from_slice(&0u16.to_le_bytes()); // Configuration
    b.extend_from_slice(&0u16.to_le_bytes()); // VDHVersion
    b.extend_from_slice(&0u16.to_le_bytes()); // Flags
    b.extend_from_slice(&0u32.to_le_bytes()); // HWBufferSize
    b.extend_from_slice(&0u32.to_le_bytes()); // FullSaveSize
    b.extend_from_slice(&0u32.to_le_bytes()); // PartSaveSize
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

static ANSI: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

pub fn vio_get_ansi(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), ANSI.load(core::sync::atomic::Ordering::Relaxed) as u16);
    0
}

pub fn vio_set_ansi(c: &mut Ctx) -> u32 {
    ANSI.store(c.arg(0) != 0, core::sync::atomic::Ordering::Relaxed);
    0
}

pub fn vio_get_state(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let Some(hdr) = c.read_vec(p, 4) else { return err::INVALID_ADDRESS };
    let ty = u16::from_le_bytes([hdr[2], hdr[3]]);
    match ty {
        // VIOINTENSITY: background intensity instead of blink.
        2 => {
            c.write_u16(p + 4, 1);
            0
        }
        // VIOOVERSCAN (border colour).
        1 => {
            c.write_u16(p + 4, 0);
            0
        }
        // Palette registers: report the standard identity palette.
        0 => {
            let Some(first) = c.read_u16(p + 4) else { return err::INVALID_ADDRESS };
            let cb = u16::from_le_bytes([hdr[0], hdr[1]]) as u32;
            let n = (cb.saturating_sub(6)) / 2;
            for i in 0..n {
                c.write_u16(p + 6 + i * 2, (first as u32 + i) as u16);
            }
            0
        }
        _ => 0,
    }
}

pub fn vio_set_state(_c: &mut Ctx) -> u32 {
    0
}

pub fn vio_get_buf(c: &mut Ctx) -> u32 {
    let (rows, cols) = rows_cols();
    let len = (rows * cols * 2) as u32;
    let base = match lvb(c, len) {
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

/// The process's logical video buffer (allocated on first use).
fn lvb(c: &Ctx, len: u32) -> Result<u32, u32> {
    let mut slot = c.process.lvb.lock();
    if *slot != 0 {
        return Ok(*slot);
    }
    let r = c.space().alloc(len.max(0x2000), pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None);
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
    let Some(data) = c.read_vec(base + (off & !1), len.max(2) & !1) else { return err::INVALID_ADDRESS };
    screen::with(|s| {
        let cols = s.cols;
        for (i, ch) in data.chunks(2).enumerate() {
            let pos = (off as usize / 2) + i;
            let (r, col) = (pos / cols, pos % cols);
            if r >= s.rows {
                break;
            }
            s.put(r, col, ch[0] as u16 | (ch[1] as u16) << 8);
        }
    });
    0
}

pub fn vio_scr_lock(c: &mut Ctx) -> u32 {
    c.write_u8(c.ptr(1), 0); // lock obtained
    0
}

pub fn vio_pop_up(_c: &mut Ctx) -> u32 {
    0
}

pub fn ok(_c: &mut Ctx) -> u32 {
    0
}
