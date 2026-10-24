//! AVIO: VIO presentation spaces shown in windows.
//!
//! The kernel keeps a program's VIO presentation spaces (VioCreatePS) and
//! records which of their cells change (the VIO calls on a PS, VioShowBuf,
//! VioShowPS). A PS associated with a window's device context
//! (VioAssociate with WinOpenWindowDC's handle) is drawn here: as the
//! program goes back to its message loop and as it ends a paint, PMWIN
//! asks the kernel (`Rs2AvioQuery`) for changed presentation spaces and
//! draws their cells with the VIO font (crate `viocell`), the PS's origin
//! (VioSetOrg) at the window's top left.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use viocell::{Cursor, Font, Grid, Style};

use crate::gpiapi::PointL;

unsafe extern "C" {
    #[link_name = "RS2API.500"]
    fn Rs2AvioQuery(hdc: u32, info: *mut Info, cells: *mut u16, cb: u32) -> u32;
    #[link_name = "RS2API.501"]
    fn Rs2AvioFont(cy: u32, glyphs: *mut u8, cb: u32) -> u32;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Info {
    hdc: u32,
    hvps: u16,
    rows: u16,
    cols: u16,
    cb: u16,
    org_row: u16,
    org_col: u16,
    cur_row: u16,
    cur_col: u16,
    cur_start: u16,
    cur_end: u16,
    cur_attr: u16,
    cell_w: u16,
    cell_h: u16,
    r0: u16,
    c0: u16,
    r1: u16,
    c1: u16,
}

/// The process opened a window DC (a PS can only be associated with one).
static ANY_DC: AtomicBool = AtomicBool::new(false);

/// The CGA colours.
const PALETTE: [u32; 16] = [
    0x000000, 0x000080, 0x008000, 0x008080, 0x800000, 0x800080, 0x808000, 0xc0c0c0, 0x808080, 0x0000ff, 0x00ff00, 0x00ffff, 0xff0000, 0xff00ff, 0xffff00, 0xffffff,
];

pub fn dc_opened() {
    ANY_DC.store(true, Ordering::Relaxed);
}

/// Fonts by cell height (8, 14, 16).
static mut FONTS: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];

fn font(cy: usize) -> Option<&'static [u8]> {
    let i = match cy {
        8 => 0,
        14 => 1,
        16 => 2,
        _ => return None,
    };
    let f = unsafe { &mut (*core::ptr::addr_of_mut!(FONTS))[i] };
    if f.is_empty() {
        let mut g = alloc::vec![0u8; 256 * cy];
        if unsafe { Rs2AvioFont(cy as u32, g.as_mut_ptr(), g.len() as u32) } != 0 {
            return None;
        }
        *f = g;
    }
    Some(f)
}

/// Draw what changed in the caller's presentation spaces.
pub fn poll() {
    if !ANY_DC.load(Ordering::Relaxed) {
        return;
    }
    // A few rounds at most: a PS changed while it is drawn waits.
    for _ in 0..8 {
        let mut info = Info::default();
        if unsafe { Rs2AvioQuery(0, &mut info, core::ptr::null_mut(), 0) } != 0 {
            return;
        }
        let Some(h) = crate::gpiobj::window_of_dc(info.hdc) else { continue };
        let n = info.rows as usize * info.cols as usize;
        let mut cells = alloc::vec![0u16; n];
        let mut now = Info::default();
        if unsafe { Rs2AvioQuery(info.hdc, &mut now, cells.as_mut_ptr(), (n * 2) as u32) } != 0 {
            continue;
        }
        draw(h, &now, &cells, (info.r0 as usize, info.c0 as usize, info.r1 as usize, info.c1 as usize));
    }
}

/// Cells r0..=r1 x c0..=c1 of a PS into window `h`.
fn draw(h: u32, i: &Info, cells: &[u16], (r0, c0, r1, c1): (usize, usize, usize, usize)) {
    let (rows, cols) = (i.rows as usize, i.cols as usize);
    let (or, oc) = (i.org_row as usize, i.org_col as usize);
    let Some(glyphs) = font(i.cell_h as usize) else { return };
    let fh = i.cell_h as usize;
    let Some(w) = crate::state::st().desk.get(h) else { return };
    let (ww, wh) = (w.cx, w.cy);
    // Only what the window shows.
    let vis_rows = (wh.max(0) as usize).div_ceil(fh);
    let vis_cols = (ww.max(0) as usize).div_ceil(viocell::CELL_W);
    let (r0, c0) = (r0.max(or), c0.max(oc));
    let (r1, c1) = ((r1 + 1).min(rows).min(or + vis_rows), (c1 + 1).min(cols).min(oc + vis_cols));
    if r0 >= r1 || c0 >= c1 {
        return;
    }
    let cursor = (i.cur_attr != 0xffff && crate::win::WinQueryFocus(1) == h).then(|| {
        let line = |v: u16| -> usize {
            let v = v as i16;
            // Negative: percent of the cell.
            if v < 0 { ((-v) as usize * fh / 100).saturating_sub(1) } else { (v as usize).min(fh - 1) }
        };
        let (a, b) = (line(i.cur_start), line(i.cur_end));
        Cursor { row: i.cur_row as usize, col: i.cur_col as usize, first: a.min(b), last: a.max(b) }
    });
    // Attribute bit 7 brightens the background (windows do not blink).
    let style = Style { palette: PALETTE, blink: false, blink_off: false, cursor, mark: None };
    let grid = Grid { cells, cols, rows };
    let width = (c1 - c0) * viocell::CELL_W;
    let height = (r1 - r0) * fh;
    let stride = viocell::bgr24_stride(width);
    let mut buf = alloc::vec![0u8; stride * height];
    viocell::render_bgr24(&grid, &Font { glyphs, height: fh }, &style, r0..r1, c0..c1, &mut buf, stride, true);
    let mut bmi = [0u8; 12];
    bmi[0] = 12;
    bmi[4..6].copy_from_slice(&(width as u16).to_le_bytes());
    bmi[6..8].copy_from_slice(&(height as u16).to_le_bytes());
    bmi[8] = 1;
    bmi[10] = 24;
    let x = ((c0 - oc) * viocell::CELL_W) as i32;
    let top = wh - ((r0 - or) * fh) as i32;
    let pts = [PointL { x, y: top - height as i32 }, PointL { x: x + width as i32 - 1, y: top - 1 }, PointL { x: 0, y: 0 }];
    let hps = crate::win::WinGetPS(h);
    crate::gpiobj::GpiDrawBits(hps, buf.as_ptr(), bmi.as_ptr(), 3, pts.as_ptr(), 0xcc, 0);
    crate::win::WinReleasePS(hps);
}
