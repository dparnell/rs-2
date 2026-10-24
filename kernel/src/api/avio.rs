//! AVIO: VIO presentation spaces of Presentation Manager programs.
//!
//! A PM program makes a presentation space of character cells
//! (VioCreatePS), associates it with a window's device context
//! (VioAssociate) and then uses the ordinary VIO calls with the PS's
//! handle as their last argument: they work on its cells instead of the
//! session's screen. The cells are in the program's own memory (VioGetBuf
//! gives the program their address, VioShowBuf/VioShowPS show what it
//! wrote there); what changes is recorded, and PMWIN, which draws the
//! window, asks for it (Rs2AvioQuery) as the program goes back to its
//! message loop or ends a paint.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::sync::SpinLock;
use crate::vm::{self, pag, SegKind};

const ERROR_VIO_INVALID_HANDLE: u32 = 436;
const ERROR_VIO_INVALID_PARMS: u32 = 421;
const ERROR_VIO_ROW: u32 = 358;
const ERROR_VIO_COL: u32 = 359;
const ERROR_VIO_DEVICE_CELL_SIZE: u32 = 498;

/// FORMAT_CGA (two bytes a cell) and FORMAT_4BYTE (character, attribute,
/// extended attribute, reserved).
const FORMAT_4BYTE: u32 = 3;

pub struct Ps {
    pid: u32,
    rows: usize,
    cols: usize,
    /// Bytes a cell (2 or 4).
    cb: usize,
    /// The cells in the process's memory.
    base: u32,
    cur: (usize, usize),
    cur_type: [u16; 4],
    org: (usize, usize),
    hdc: u32,
    cell: (u16, u16),
    /// Changed cells not drawn yet: rows r0..=r1, columns c0..=c1.
    dirty: Option<(usize, usize, usize, usize)>,
    /// Logical fonts (lcid -> FATTRS face).
    fonts: Vec<(u32, Vec<u8>)>,
}

static PS: SpinLock<BTreeMap<u16, Ps>> = SpinLock::new(BTreeMap::new());
static NEXT: SpinLock<u16> = SpinLock::new(0x100);

impl Ps {
    fn mark(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        let (r1, c1) = (r1.min(self.rows - 1), c1.min(self.cols - 1));
        if r0 > r1 || c0 > c1 {
            return;
        }
        self.dirty = Some(match self.dirty {
            Some((a, b, c, d)) => (a.min(r0), b.min(c0), c.max(r1), d.max(c1)),
            None => (r0, c0, r1, c1),
        });
    }

    fn all(&mut self) {
        self.mark(0, 0, self.rows - 1, self.cols - 1);
    }

    fn addr(&self, r: usize, col: usize) -> u32 {
        self.base + ((r * self.cols + col) * self.cb) as u32
    }
}

/// The caller's PS with handle `h` (the last VIO argument), if it is one.
fn with<R>(c: &Ctx, h: u32, f: impl FnOnce(&mut Ps) -> R) -> Option<R> {
    let pid = c.process.pid;
    let mut t = PS.lock();
    let ps = t.get_mut(&(h as u16)).filter(|p| p.pid == pid)?;
    Some(f(ps))
}

/// The handle in argument `i` names an AVIO PS of the caller.
pub fn is_ps(c: &Ctx, i: usize) -> Option<u32> {
    let h = c.arg(i) & 0xffff;
    if h == 0 {
        return None;
    }
    let pid = c.process.pid;
    PS.lock().get(&(h as u16)).filter(|p| p.pid == pid).map(|_| h)
}

/// The PS's geometry and cell size (the lock is not held while user
/// memory is touched).
fn geom(c: &Ctx, h: u32) -> Option<(usize, usize, usize, u32)> {
    with(c, h, |p| (p.rows, p.cols, p.cb, p.base))
}

fn read_cell(c: &Ctx, base: u32, cols: usize, cb: usize, r: usize, col: usize) -> u16 {
    c.read_u16(base + ((r * cols + col) * cb) as u32).unwrap_or(0x0720)
}

fn write_cell(c: &Ctx, base: u32, cols: usize, cb: usize, r: usize, col: usize, v: u16) {
    c.write_u16(base + ((r * cols + col) * cb) as u32, v);
}

// ---- The AVIO calls ----

/// VioCreatePS(phvps, sdepth, swidth, sFormat, sAttrs, hvpsReserved).
pub fn create_ps(c: &mut Ctx) -> u32 {
    let (p, rows, cols, fmt, attrs) = (c.ptr(0), c.arg(1) as i16, c.arg(2) as i16, c.arg(3), c.arg(4));
    let cb = if fmt == FORMAT_4BYTE { 4 } else { 2 };
    if rows <= 0 || cols <= 0 || (rows as usize * cols as usize * cb) > 0x10000 || !(1..=3).contains(&attrs) && attrs != 0 {
        return ERROR_VIO_INVALID_PARMS;
    }
    let (rows, cols) = (rows as usize, cols as usize);
    let size = (rows * cols * cb) as u32;
    let base = match c.space().alloc(size.next_multiple_of(0x1000), pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None) {
        Ok(b) => b,
        Err(e) => return e.code(),
    };
    // Blanks, light grey on black.
    let blank: Vec<u8> = (0..rows * cols).flat_map(|_| if cb == 4 { alloc::vec![b' ', 7, 0, 0] } else { alloc::vec![b' ', 7] }).collect();
    c.write(base, &blank);
    let h = {
        let mut n = NEXT.lock();
        *n = n.wrapping_add(1).max(0x100);
        *n
    };
    let ps = Ps { pid: c.process.pid, rows, cols, cb, base, cur: (0, 0), cur_type: [14, 15, 1, 0], org: (0, 0), hdc: 0, cell: (8, 16), dirty: None, fonts: Vec::new() };
    PS.lock().insert(h, ps);
    c.write_u16(p, h);
    0
}

/// VioDestroyPS(hvps).
pub fn destroy_ps(c: &mut Ctx) -> u32 {
    let h = c.arg(0) as u16;
    let pid = c.process.pid;
    let base = {
        let mut t = PS.lock();
        match t.get(&h) {
            Some(p) if p.pid == pid => t.remove(&h).map(|p| p.base),
            _ => None,
        }
    };
    match base {
        Some(b) => {
            let _ = c.space().free(b);
            0
        }
        None => ERROR_VIO_INVALID_HANDLE,
    }
}

/// VioAssociate(hdc, hvps): hdc 0 takes the PS off its window.
pub fn associate(c: &mut Ctx) -> u32 {
    let (hdc, h) = (c.arg(0), c.arg(1));
    with(c, h, |p| {
        p.hdc = hdc;
        p.all();
    })
    .map_or(ERROR_VIO_INVALID_HANDLE, |_| 0)
}

/// VioShowPS(sDepth, sWidth, soffCell, hvps): the cells from cell offset
/// soffCell, sDepth rows of sWidth columns, are to be drawn.
pub fn show_ps(c: &mut Ctx) -> u32 {
    let (depth, width, off, h) = (c.arg(0) as i16 as isize, c.arg(1) as i16 as isize, c.arg(2) as i16 as isize, c.arg(3));
    with(c, h, |p| {
        if depth <= 0 || width <= 0 || off < 0 {
            return ERROR_VIO_INVALID_PARMS;
        }
        let (r0, c0) = (off as usize / p.cols, off as usize % p.cols);
        p.mark(r0, c0, r0 + depth as usize - 1, c0 + width as usize - 1);
        0
    })
    .unwrap_or(ERROR_VIO_INVALID_HANDLE)
}

/// VioSetOrg(sRow, sColumn, hvps): the cell shown at the window's top
/// left.
pub fn set_org(c: &mut Ctx) -> u32 {
    let (r, col, h) = (c.arg(0) as i16, c.arg(1) as i16, c.arg(2));
    with(c, h, |p| {
        if r < 0 || col < 0 || r as usize >= p.rows || col as usize >= p.cols {
            return ERROR_VIO_INVALID_PARMS;
        }
        p.org = (r as usize, col as usize);
        p.all();
        0
    })
    .unwrap_or(ERROR_VIO_INVALID_HANDLE)
}

pub fn get_org(c: &mut Ctx) -> u32 {
    let (pr, pc, h) = (c.ptr(0), c.ptr(1), c.arg(2));
    match with(c, h, |p| p.org) {
        Some((r, col)) => {
            c.write_u16(pr, r as u16);
            c.write_u16(pc, col as u16);
            0
        }
        None => ERROR_VIO_INVALID_HANDLE,
    }
}

/// VioGetDeviceCellSize(psHeight, psWidth, hvps).
pub fn get_cell_size(c: &mut Ctx) -> u32 {
    let (ph, pw, h) = (c.ptr(0), c.ptr(1), c.arg(2));
    match with(c, h, |p| p.cell) {
        Some((w, hh)) => {
            c.write_u16(ph, hh);
            c.write_u16(pw, w);
            0
        }
        None => ERROR_VIO_INVALID_HANDLE,
    }
}

/// VioSetDeviceCellSize(sHeight, sWidth, hvps): the cell sizes of rs2's
/// VIO fonts (8 wide, 8, 14 or 16 high).
pub fn set_cell_size(c: &mut Ctx) -> u32 {
    let (hh, w, h) = (c.arg(0) as u16, c.arg(1) as u16, c.arg(2));
    if w != 8 || !matches!(hh, 8 | 14 | 16) {
        return if is_ps(c, 2).is_some() { ERROR_VIO_DEVICE_CELL_SIZE } else { ERROR_VIO_INVALID_HANDLE };
    }
    with(c, h, |p| {
        p.cell = (w, hh);
        p.all();
    })
    .map_or(ERROR_VIO_INVALID_HANDLE, |_| 0)
}

/// VioCreateLogFont(pfatattrs, llcid, pstr8Name, hvps): kept (the cells
/// are drawn in the VIO font).
pub fn create_log_font(c: &mut Ctx) -> u32 {
    let (pf, lcid, h) = (c.ptr(0), c.arg(1), c.arg(3));
    let face = c.read_vec(pf + 16, 32).unwrap_or_default();
    with(c, h, |p| {
        p.fonts.retain(|f| f.0 != lcid);
        p.fonts.push((lcid, face));
    })
    .map_or(ERROR_VIO_INVALID_HANDLE, |_| 0)
}

/// VioDeleteSetId(llcid, hvps).
pub fn delete_set_id(c: &mut Ctx) -> u32 {
    let (lcid, h) = (c.arg(0), c.arg(1));
    with(c, h, |p| {
        let n = p.fonts.len();
        p.fonts.retain(|f| f.0 != lcid);
        if p.fonts.len() == n { ERROR_VIO_INVALID_PARMS } else { 0 }
    })
    .unwrap_or(ERROR_VIO_INVALID_HANDLE)
}

/// VioQuerySetIds(allcids, pNames, alTypes, lcount, hvps).
pub fn query_set_ids(c: &mut Ctx) -> u32 {
    let (pl, pn, pt, n, h) = (c.ptr(0), c.ptr(1), c.ptr(2), c.arg(3) as usize, c.arg(4));
    let Some(fonts) = with(c, h, |p| p.fonts.clone()) else { return ERROR_VIO_INVALID_HANDLE };
    for (i, (lcid, face)) in fonts.iter().take(n).enumerate() {
        c.write_u32(pl + 4 * i as u32, *lcid);
        let mut name = face.iter().take(8).copied().collect::<Vec<u8>>();
        name.resize(8, 0);
        c.write(pn + 8 * i as u32, &name);
        // LCIDT_FONT.
        c.write_u32(pt + 4 * i as u32, 6);
    }
    0
}

/// VioQueryFonts(plRemfonts, afmMetrics, lMetricsLength, plFonts,
/// pszFacename, flOptions, hvps): the VIO fonts are not GPI fonts; none.
pub fn query_fonts(c: &mut Ctx) -> u32 {
    let (prem, pn, h) = (c.ptr(0), c.ptr(3), c.arg(6));
    if is_ps(c, 6).is_none() && h != 0 {
        return ERROR_VIO_INVALID_HANDLE;
    }
    c.write_u32(prem, 0);
    c.write_u32(pn, 0);
    0
}

// ---- The VIO calls on a PS (the last argument names it) ----

pub fn get_cur_pos(c: &mut Ctx, h: u32) -> u32 {
    let Some((r, col)) = with(c, h, |p| p.cur) else { return ERROR_VIO_INVALID_HANDLE };
    c.write_u16(c.ptr(0), r as u16);
    c.write_u16(c.ptr(1), col as u16);
    0
}

pub fn set_cur_pos(c: &mut Ctx, h: u32) -> u32 {
    let (r, col) = (c.arg(0) as usize, c.arg(1) as usize);
    with(c, h, |p| {
        if r >= p.rows {
            return ERROR_VIO_ROW;
        }
        if col >= p.cols {
            return ERROR_VIO_COL;
        }
        let old = p.cur;
        p.cur = (r, col);
        p.mark(old.0, old.1, old.0, old.1);
        p.mark(r, col, r, col);
        0
    })
    .unwrap_or(ERROR_VIO_INVALID_HANDLE)
}

pub fn get_cur_type(c: &mut Ctx, h: u32) -> u32 {
    let Some(t) = with(c, h, |p| p.cur_type) else { return ERROR_VIO_INVALID_HANDLE };
    let b: Vec<u8> = t.iter().flat_map(|w| w.to_le_bytes()).collect();
    if c.write(c.ptr(0), &b) { 0 } else { err::INVALID_ADDRESS }
}

pub fn set_cur_type(c: &mut Ctx, h: u32) -> u32 {
    let Some(raw) = c.read_vec(c.ptr(0), 8) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
    with(c, h, |p| {
        p.cur_type = [w(0), w(1), w(2), w(3)];
        let (r, col) = p.cur;
        p.mark(r, col, r, col);
        0
    })
    .unwrap_or(ERROR_VIO_INVALID_HANDLE)
}

/// VioGetMode on a PS: its rows and columns.
pub fn get_mode(c: &mut Ctx, h: u32) -> u32 {
    let Some((rows, cols, cb, _)) = geom(c, h) else { return ERROR_VIO_INVALID_HANDLE };
    let p = c.ptr(0);
    let Some(len) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    let mut b = Vec::new();
    b.extend_from_slice(&34u16.to_le_bytes());
    b.push(1);
    b.push(4);
    b.extend_from_slice(&(cols as u16).to_le_bytes());
    b.extend_from_slice(&(rows as u16).to_le_bytes());
    b.extend_from_slice(&((cols * 8) as u16).to_le_bytes());
    b.extend_from_slice(&((rows * 16) as u16).to_le_bytes());
    b.push(if cb == 4 { 3 } else { 0 });
    b.push(1);
    b.extend_from_slice(&0u32.to_le_bytes());
    let n = (rows * cols * cb) as u32;
    for _ in 0..3 {
        b.extend_from_slice(&n.to_le_bytes());
    }
    b.extend_from_slice(&0u32.to_le_bytes());
    b.truncate((len as usize).clamp(2, b.len()));
    if c.write(p + 2, &b[2..]) { 0 } else { err::INVALID_ADDRESS }
}

/// Apply `f` to `n` cells from (row, col), wrapping at line ends.
fn modify_run(c: &Ctx, h: u32, n: usize, row: usize, col: usize, mut f: impl FnMut(usize, u16) -> u16) -> u32 {
    let Some((rows, cols, cb, base)) = geom(c, h) else { return ERROR_VIO_INVALID_HANDLE };
    if row >= rows {
        return ERROR_VIO_ROW;
    }
    if col >= cols {
        return ERROR_VIO_COL;
    }
    let (mut r, mut cc) = (row, col);
    let mut last = (row, col);
    for i in 0..n {
        if r >= rows {
            break;
        }
        let v = f(i, read_cell(c, base, cols, cb, r, cc));
        write_cell(c, base, cols, cb, r, cc, v);
        last = (r, cc);
        cc += 1;
        if cc >= cols {
            cc = 0;
            r += 1;
        }
    }
    if n > 0 {
        with(c, h, |p| {
            if last.0 == row {
                p.mark(row, col, row, last.1);
            } else {
                p.mark(row, 0, last.0, cols - 1);
            }
        });
    }
    0
}

pub fn wrt_char_str(c: &mut Ctx, h: u32) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    modify_run(c, h, data.len(), row, col, |i, old| (old & 0xff00) | data[i] as u16)
}

pub fn wrt_char_str_att(c: &mut Ctx, h: u32) -> u32 {
    let (p, n, row, col, pa) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize, c.ptr(4));
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    let Some(attr) = c.read_u8(pa) else { return err::INVALID_ADDRESS };
    modify_run(c, h, data.len(), row, col, |i, _| data[i] as u16 | (attr as u16) << 8)
}

pub fn wrt_cell_str(c: &mut Ctx, h: u32) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1), c.arg(2) as usize, c.arg(3) as usize);
    let cb = geom(c, h).map_or(2, |g| g.2);
    let Some(data) = c.read_vec(p, n - n % cb as u32) else { return err::INVALID_ADDRESS };
    modify_run(c, h, data.len() / cb, row, col, |i, _| data[i * cb] as u16 | (data[i * cb + 1] as u16) << 8)
}

pub fn wrt_n_cell(c: &mut Ctx, h: u32) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(cell) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    modify_run(c, h, n, row, col, |_, _| cell)
}

pub fn wrt_n_char(c: &mut Ctx, h: u32) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(ch) = c.read_u8(p) else { return err::INVALID_ADDRESS };
    modify_run(c, h, n, row, col, |_, old| (old & 0xff00) | ch as u16)
}

pub fn wrt_n_attr(c: &mut Ctx, h: u32) -> u32 {
    let (p, n, row, col) = (c.ptr(0), c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize);
    let Some(a) = c.read_u8(p) else { return err::INVALID_ADDRESS };
    modify_run(c, h, n, row, col, |_, old| (old & 0x00ff) | (a as u16) << 8)
}

/// VioReadCellStr / VioReadCharStr on a PS.
pub fn read_run(c: &mut Ctx, h: u32, cells: bool) -> u32 {
    let (p, pcb, row, col) = (c.ptr(0), c.ptr(1), c.arg(2) as usize, c.arg(3) as usize);
    let Some((rows, cols, cb, base)) = geom(c, h) else { return ERROR_VIO_INVALID_HANDLE };
    let Some(want) = c.read_u16(pcb) else { return err::INVALID_ADDRESS };
    if row >= rows {
        return ERROR_VIO_ROW;
    }
    if col >= cols {
        return ERROR_VIO_COL;
    }
    let per = if cells { cb } else { 1 };
    let mut v = Vec::new();
    let (mut r, mut cc) = (row, col);
    for _ in 0..want as usize / per {
        if r >= rows {
            break;
        }
        let a = base + ((r * cols + cc) * cb) as u32;
        let cell = c.read_vec(a, per as u32).unwrap_or_default();
        v.extend_from_slice(&cell);
        cc += 1;
        if cc >= cols {
            cc = 0;
            r += 1;
        }
    }
    c.write(p, &v);
    c.write_u16(pcb, v.len() as u16);
    0
}

/// VioScrollUp/Dn/Lf/Rt on a PS: 0 up, 1 down, 2 left, 3 right.
pub fn scroll(c: &mut Ctx, h: u32, dir: u8) -> u32 {
    let (top, left, bottom, right, n) = (c.arg(0) as usize, c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize, c.arg(4) as usize);
    let Some(fill) = c.read_u16(c.ptr(5)) else { return err::INVALID_ADDRESS };
    let Some((rows, cols, cb, base)) = geom(c, h) else { return ERROR_VIO_INVALID_HANDLE };
    if top >= rows {
        return ERROR_VIO_ROW;
    }
    if left >= cols {
        return ERROR_VIO_COL;
    }
    let (bottom, right) = (bottom.min(rows - 1), right.min(cols - 1));
    if top > bottom || left > right || n == 0 {
        return 0;
    }
    let (hh, w) = (bottom - top + 1, right - left + 1);
    let n = n.min(if dir < 2 { hh } else { w });
    let mut buf = Vec::with_capacity(hh * w);
    for r in top..=bottom {
        for col in left..=right {
            buf.push(read_cell(c, base, cols, cb, r, col));
        }
    }
    let get = |r: isize, col: isize| -> u16 { if r < 0 || col < 0 || r as usize >= hh || col as usize >= w { fill } else { buf[r as usize * w + col as usize] } };
    for r in 0..hh {
        for col in 0..w {
            let (sr, sc) = match dir {
                0 => (r as isize + n as isize, col as isize),
                1 => (r as isize - n as isize, col as isize),
                2 => (r as isize, col as isize + n as isize),
                _ => (r as isize, col as isize - n as isize),
            };
            write_cell(c, base, cols, cb, top + r, left + col, get(sr, sc));
        }
    }
    with(c, h, |p| p.mark(top, left, bottom, right));
    0
}

/// VioWrtTTY on a PS: characters at the cursor, which moves on; CR, LF,
/// backspace, tab and bell; the PS scrolls up at its end.
pub fn wrt_tty(c: &mut Ctx, h: u32) -> u32 {
    let (p, n) = (c.ptr(0), c.arg(1));
    let Some(data) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    let Some((rows, cols, cb, base)) = geom(c, h) else { return ERROR_VIO_INVALID_HANDLE };
    let Some((mut r, mut col)) = with(c, h, |p| p.cur) else { return ERROR_VIO_INVALID_HANDLE };
    let (r_start, mut r_min) = (r, r);
    let mut scrolled = false;
    for &b in &data {
        match b {
            b'\r' => col = 0,
            b'\n' => r += 1,
            8 => col = col.saturating_sub(1),
            7 => {}
            b'\t' => col = ((col / 8) + 1) * 8,
            _ => {
                let old = read_cell(c, base, cols, cb, r, col);
                write_cell(c, base, cols, cb, r, col, (old & 0xff00) | b as u16);
                col += 1;
            }
        }
        if col >= cols {
            col = 0;
            r += 1;
        }
        if r >= rows {
            // Up one line; the new line keeps the attribute of the last.
            let attr = read_cell(c, base, cols, cb, rows - 1, cols - 1) & 0xff00;
            for rr in 1..rows {
                for cc in 0..cols {
                    let v = read_cell(c, base, cols, cb, rr, cc);
                    write_cell(c, base, cols, cb, rr - 1, cc, v);
                }
            }
            for cc in 0..cols {
                write_cell(c, base, cols, cb, rows - 1, cc, attr | b' ' as u16);
            }
            r = rows - 1;
            scrolled = true;
            r_min = 0;
        }
    }
    with(c, h, |p| {
        p.cur = (r, col);
        if scrolled {
            p.all();
        } else {
            p.mark(r_start.min(r_min), 0, r, cols - 1);
        }
    });
    0
}

/// VioGetBuf on a PS: the address (16:16) and size of its cells.
pub fn get_buf(c: &mut Ctx, h: u32) -> u32 {
    let Some((rows, cols, cb, base)) = geom(c, h) else { return ERROR_VIO_INVALID_HANDLE };
    c.write_u32(c.ptr(0), (vm::tile_selector(base) as u32) << 16);
    c.write_u16(c.ptr(1), (rows * cols * cb) as u16);
    0
}

/// VioShowBuf(offLVB, cb, hvps) on a PS: the cells in that range are to
/// be drawn.
pub fn show_buf(c: &mut Ctx, h: u32) -> u32 {
    let (off, n) = (c.arg(0) as usize, c.arg(1) as usize);
    with(c, h, |p| {
        let first = off / p.cb;
        let last = (off + n.max(1) - 1) / p.cb;
        let (r0, r1) = (first / p.cols, last / p.cols);
        if r0 == r1 {
            p.mark(r0, first % p.cols, r1, last % p.cols);
        } else {
            p.mark(r0, 0, r1, p.cols - 1);
        }
        0
    })
    .unwrap_or(ERROR_VIO_INVALID_HANDLE)
}

// ---- PMWIN's side (RS2API) ----

/// Rs2AvioQuery(hdc, pInfo, pCells, cbCells): the PS associated with a
/// device context of the caller: AVIOINFO (hvps, rows, cols, cell bytes,
/// org row, org col, cursor row, col, cursor start, end, attribute (-1
/// hidden), cell width, height, dirty r0, c0, r1, c1 (r0 = 0xffff: none),
/// all USHORTs) and, when pCells is given, the cells as char | attr << 8
/// (USHORTs, row by row). The changes are cleared. hdc 0: the first PS of
/// the caller with changes (its hdc goes into the info's first ULONG).
pub fn query(c: &mut Ctx) -> u32 {
    let (hdc, pi, pc, cbc) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3) as usize);
    let pid = c.process.pid;
    let found = {
        let mut t = PS.lock();
        let e = t.iter_mut().find(|(_, p)| p.pid == pid && p.hdc != 0 && if hdc == 0 { p.dirty.is_some() } else { p.hdc == hdc });
        e.map(|(&h, p)| {
            let d = p.dirty.take().unwrap_or((0xffff, 0, 0, 0));
            let info = [
                h,
                p.rows as u16,
                p.cols as u16,
                p.cb as u16,
                p.org.0 as u16,
                p.org.1 as u16,
                p.cur.0 as u16,
                p.cur.1 as u16,
                p.cur_type[0],
                p.cur_type[1],
                p.cur_type[3],
                p.cell.0,
                p.cell.1,
                d.0 as u16,
                d.1 as u16,
                d.2 as u16,
                d.3 as u16,
            ];
            (info, p.hdc, p.base, p.rows * p.cols, p.cb)
        })
    };
    let Some((info, phdc, base, n, cb)) = found else { return ERROR_VIO_INVALID_HANDLE };
    let mut b: Vec<u8> = phdc.to_le_bytes().to_vec();
    for v in info {
        b.extend_from_slice(&v.to_le_bytes());
    }
    c.write(pi, &b);
    if pc != 0 {
        let raw = c.read_vec(base, (n * cb) as u32).unwrap_or_default();
        let cells: Vec<u8> = raw.chunks(cb).take(cbc / 2).flat_map(|x| [x[0], x[1]]).collect();
        c.write(pc, &cells);
    }
    0
}

/// Rs2AvioFont(cy, pGlyphs, cb): the VIO font for cells `cy` high (8, 14
/// or 16), 256 glyphs of `cy` bytes.
pub fn font(c: &mut Ctx) -> u32 {
    let (cy, p, cb) = (c.arg(0) as usize, c.arg(1), c.arg(2) as usize);
    if !matches!(cy, 8 | 14 | 16) || cb < 256 * cy {
        return err::INVALID_PARAMETER;
    }
    let cp = crate::screen::with(|s| s.codepage()).unwrap_or(crate::codepage::CodePage::Cp850);
    let f = crate::screen::rom_font(cp, cy);
    if c.write(p, &f) { 0 } else { err::INVALID_ADDRESS }
}

/// The process ended: its presentation spaces go.
pub fn process_ended(pid: u32) {
    PS.lock().retain(|_, p| p.pid != pid);
}
