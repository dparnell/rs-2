//! RS2DISP.DRV, rs2's paravirtual Windows 3.1 display driver
//! (docs/WINOS2.md): the driver's NE module (crate `win16drv`) has one
//! trap per exported entry point; this module does the work. Windows'
//! GDI calls the entries with the Pascal convention: the arguments are
//! on the stack above the far return address, the first one highest;
//! the result goes in DX:AX and the stub's `RETF n` pops them.
//!
//! The screen is a linear 8-bit surface of the machine's own
//! (`Display`), with a fixed palette laid out as Windows lays out its
//! system palette (the 20 static colours at 0-9 and 246-255, so that
//! inverting an index inverts the colour), a 6x6x6 colour cube and
//! greys in between. Memory bitmaps are GDI's (1 bpp, or 8 bpp in the
//! screen's format) in guest memory. GDI simulates what the driver's
//! GDIINFO does not claim: curves, polygons and wide lines come as scan
//! lines and polylines.
//!
//! What it draws is shown by the host (`Host::present_display`) with
//! the rectangles that changed; the pointer shape and position go to the
//! host too (`Host::pointer`), which makes it the pointer of its window.

mod blt;
mod dib;
mod draw;
mod text;

use alloc::vec;
use alloc::vec::Vec;

pub use blt::{rop2, rop3};
pub use win16drv::{CB_DISP, CB_DISP_LAST};
use x86emu::SS;

use crate::{Host, Machine};

/// A rectangle, right and bottom exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    fn area(&self) -> i64 {
        if self.is_empty() { 0 } else { (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64 }
    }
}

/// The pointer: hot spot, size and the AND and XOR masks (1 bit per
/// pixel, `width_bytes` per row, top row first).
#[derive(Clone, Debug, Default)]
pub struct Pointer {
    pub hot: (i32, i32),
    pub w: usize,
    pub h: usize,
    pub width_bytes: usize,
    pub and: Vec<u8>,
    pub xor: Vec<u8>,
}

/// The screen RS2DISP draws.
pub struct Display {
    /// Between ENABLE and DISABLE.
    pub enabled: bool,
    pub w: usize,
    pub h: usize,
    /// 8-bit pixels, `w` per row.
    pub pix: Vec<u8>,
    /// 0x00RRGGBB.
    pub pal: [u32; 256],
    /// What changed since the host last showed it.
    pub damage: Vec<Rect>,
    /// The pointer shape (None: hidden) and where its hot spot is; the
    /// generation changes with the shape.
    pub pointer: Option<Pointer>,
    pub pointer_pos: (i32, i32),
    pub pointer_gen: u32,
    /// SAVESCREENBITMAP's copy.
    saved: Option<(Rect, Vec<u8>)>,
    /// Log every call (debugging).
    pub trace: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display { enabled: false, w: 0, h: 0, pix: Vec::new(), pal: PALETTE, damage: Vec::new(), pointer: None, pointer_pos: (0, 0), pointer_gen: 0, saved: None, trace: false }
    }
}

impl Display {
    /// Note that `r` changed.
    pub fn touch(&mut self, r: Rect) {
        let r = r.intersect(&Rect::new(0, 0, self.w as i32, self.h as i32));
        if r.is_empty() {
            return;
        }
        for d in self.damage.iter_mut() {
            let u = d.union(&r);
            // Merge with a rectangle it overlaps or nearly touches.
            if u.area() <= d.area() + r.area() + 64 * 64 {
                *d = u;
                return;
            }
        }
        if self.damage.len() >= 16 {
            let all = self.damage.iter().fold(r, |a, d| a.union(d));
            self.damage.clear();
            self.damage.push(all);
            return;
        }
        self.damage.push(r);
    }

    pub fn take_damage(&mut self) -> Vec<Rect> {
        core::mem::take(&mut self.damage)
    }
}

/// The fixed palette: Windows' 20 static colours at 0-9 and 246-255,
/// a 6x6x6 cube in pairs (i, 255 - i) of inverse colours, greys in the
/// middle.
pub const PALETTE: [u32; 256] = {
    let mut p = [0u32; 256];
    let low: [u32; 10] = [0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0, 0xc0dcc0, 0xa6caf0];
    let high: [u32; 10] = [0xfffbf0, 0xa0a0a4, 0x808080, 0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff];
    let mut i = 0;
    while i < 10 {
        p[i] = low[i];
        p[246 + i] = high[i];
        i += 1;
    }
    // The cube: (r, g, b) at 10 + k and its inverse (5-r, 5-g, 5-b) at
    // 245 - k, for the 108 colours with r < 3 (or r=..: their inverses
    // have r >= 3).
    let lv = [0u32, 0x33, 0x66, 0x99, 0xcc, 0xff];
    let mut k = 0;
    let mut r = 0;
    while r < 3 {
        let mut g = 0;
        while g < 6 {
            let mut b = 0;
            while b < 6 {
                p[10 + k] = lv[r] << 16 | lv[g] << 8 | lv[b];
                p[245 - k] = lv[5 - r] << 16 | lv[5 - g] << 8 | lv[5 - b];
                k += 1;
                b += 1;
            }
            g += 1;
        }
        r += 1;
    }
    // 118..137: 20 greys, light and dark in inverse pairs.
    let mut j = 0;
    while j < 10 {
        let v = 0x0b + j as u32 * 0x0c;
        p[118 + j] = v << 16 | v << 8 | v;
        let w = 255 - v;
        p[137 - j] = w << 16 | w << 8 | w;
        j += 1;
    }
    p
};

/// The palette index nearest to an RGB colour (COLORREF: 0x00BBGGRR).
pub fn nearest(cref: u32) -> u8 {
    let (r, g, b) = ((cref & 0xff) as i32, ((cref >> 8) & 0xff) as i32, ((cref >> 16) & 0xff) as i32);
    let mut best = (i32::MAX, 0u8);
    for (i, &c) in PALETTE.iter().enumerate() {
        let (pr, pg, pb) = ((c >> 16) as i32 & 0xff, (c >> 8) as i32 & 0xff, c as i32 & 0xff);
        let d = (pr - r) * (pr - r) * 3 + (pg - g) * (pg - g) * 4 + (pb - b) * (pb - b) * 2;
        if d < best.0 {
            best = (d, i as u8);
            if d == 0 {
                break;
            }
        }
    }
    best.1
}

/// COLORREF of a palette index.
pub fn cref_of(i: u8) -> u32 {
    let c = PALETTE[i as usize];
    (c >> 16) & 0xff | (c & 0xff00) | (c & 0xff) << 16
}

/// The mono bit for a colour: 1 (white) when it is light.
pub fn mono_of(cref: u32) -> u8 {
    let (r, g, b) = (cref & 0xff, (cref >> 8) & 0xff, (cref >> 16) & 0xff);
    ((r * 30 + g * 59 + b * 11) >= 128 * 100) as u8
}

/// A physical colour: the index in the low byte, the mono bit in bit 8
/// (so it serves colour and monochrome destinations alike).
pub fn phys(cref: u32) -> u32 {
    let i = nearest(cref);
    i as u32 | (mono_of(cref_of(i)) as u32) << 8
}

/// The PDEVICE GDI keeps for the screen: a BITMAP-like header with a
/// nonzero type.
const PDEVICE_TYPE: u16 = 0x5232;
const PDEVICE_SIZE: u16 = 32;

/// Argument reader: the argument block above the return address, read
/// from its top (the first argument) down.
pub(crate) struct Args {
    b: Vec<u8>,
    top: usize,
}

impl Args {
    pub fn w(&mut self) -> u16 {
        self.top -= 2;
        u16::from_le_bytes([self.b[self.top], self.b[self.top + 1]])
    }
    pub fn i(&mut self) -> i32 {
        self.w() as i16 as i32
    }
    pub fn d(&mut self) -> u32 {
        self.top -= 4;
        u32::from_le_bytes([self.b[self.top], self.b[self.top + 1], self.b[self.top + 2], self.b[self.top + 3]])
    }
    /// A far pointer: (selector, offset); (0, 0) for NULL.
    pub fn p(&mut self) -> Ptr {
        let v = self.d();
        Ptr { sel: (v >> 16) as u16, off: v as u16 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ptr {
    pub sel: u16,
    pub off: u16,
}

impl Ptr {
    pub fn null(&self) -> bool {
        self.sel == 0
    }
}

/// A device to draw on: the screen or a memory bitmap.
#[derive(Clone, Debug)]
pub(crate) struct Dev {
    pub screen: bool,
    pub w: i32,
    pub h: i32,
    pub mono: bool,
    /// Memory bitmaps: bytes per row, rows per 64 KB segment and each
    /// segment's linear base.
    pub stride: usize,
    pub scan_seg: usize,
    pub segs: Vec<u32>,
}

impl Dev {
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }
    fn row_lin(&self, y: i32) -> u32 {
        let s = y as usize / self.scan_seg.max(1);
        let r = y as usize % self.scan_seg.max(1);
        self.segs.get(s).copied().unwrap_or(0).wrapping_add((r * self.stride) as u32)
    }
}

/// The physical colours of a DRAWMODE and its modes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawMode {
    pub rop2: u8,
    /// TRANSPARENT (1) or OPAQUE (2).
    pub bk_mode: u16,
    pub bk: u32,
    pub text: u32,
    pub break_extra: i32,
    pub break_err: i32,
    pub break_rem: i32,
    pub break_count: i32,
    pub char_extra: i32,
}

impl Default for DrawMode {
    fn default() -> Self {
        DrawMode { rop2: 13, bk_mode: 2, bk: 0x1ff, text: 0, break_extra: 0, break_err: 0, break_rem: 0, break_count: 0, char_extra: 0 }
    }
}

impl Machine {
    /// Linear address of a far pointer (protected mode).
    pub(crate) fn far(&mut self, p: Ptr) -> u32 {
        self.dpmi_lin(p.sel, p.off as u32)
    }

    fn disp_args(&mut self, size: u16) -> Args {
        let sp = self.cpu.stack_ptr();
        let a = self.cpu.lin(SS, sp.wrapping_add(4));
        let mut b = vec![0u8; size as usize];
        self.hw.read_bytes(a, &mut b);
        Args { top: b.len(), b }
    }

    /// An entry point of RS2DISP.DRV.
    pub(crate) fn disp_call<H: Host>(&mut self, host: &mut H, n: u16) {
        let ord = n - CB_DISP;
        let Some(&(_, _, size)) = win16drv::ENTRIES.iter().find(|e| e.0 == ord) else { return };
        let mut a = self.disp_args(size);
        if self.disp.trace {
            let mut t = alloc::format!("disp {ord}:");
            for b in a.b.chunks(2).rev() {
                t.push_str(&alloc::format!(" {:04x}", u16::from_le_bytes([b[0], b[1]])));
            }
            host.log(&t);
        }
        let r: u32 = match ord {
            1 => self.disp_bitblt(&mut a),
            2 => self.disp_colorinfo(&mut a),
            3 => 0, // CONTROL: no escapes
            4 => {
                self.disp.enabled = false;
                self.mouse.screen = None;
                1
            }
            5 => self.disp_enable(host, &mut a),
            6 | 7 => 1, // ENUMDFONTS, ENUMOBJ: none
            8 => self.disp_output(&mut a),
            9 => self.disp_pixel(&mut a),
            10 => self.disp_realize(&mut a),
            11 => self.disp_strblt(&mut a),
            12 => self.disp_scanlr(&mut a),
            13 => 1, // DEVICEMODE
            14 => self.disp_exttextout(&mut a),
            15 => self.disp_getcharwidth(&mut a),
            16 | 18 | 20 => 0, // DEVICEBITMAP, SETATTRIBUTE, CREATEBITMAP
            17 => self.disp_fastborder(&mut a),
            19 => self.disp_devicebitmapbits(&mut a),
            21 => self.disp_setdibits(&mut a),
            92 => self.disp_savescreen(&mut a),
            101 => {
                // INQUIRE: CURSORINFO (dpXRate, dpYRate).
                let p = a.p();
                let l = self.far(p);
                self.hw.wr16(l, 1);
                self.hw.wr16(l + 2, 1);
                4
            }
            102 => self.disp_setcursor(&mut a),
            103 => {
                let y = a.i();
                let x = a.i();
                self.disp.pointer_pos = (x, y);
                0
            }
            104 | 500 => 0, // CHECKCURSOR, USERREPAINTDISABLE
            _ => 0,
        };
        self.cpu.set_ax(r as u16);
        self.cpu.set_dx((r >> 16) as u16);
    }

    /// ENABLE(lpDevInfo, wStyle, lpDestDevType, lpOutputFile, lpData):
    /// with wStyle bit 0 the GDIINFO, else the PDEVICE (and the screen).
    fn disp_enable<H: Host>(&mut self, host: &mut H, a: &mut Args) -> u32 {
        let info = a.p();
        let style = a.w();
        let (w, h) = host.display_size();
        let (w, h) = (w.clamp(640, 1600) as usize, h.clamp(480, 1200) as usize);
        let at = self.far(info);
        if style & 1 != 0 {
            let g = gdiinfo(w, h);
            self.hw.write_bytes(at, &g);
            return g.len() as u32;
        }
        let mut pd = [0u8; PDEVICE_SIZE as usize];
        pd[0..2].copy_from_slice(&PDEVICE_TYPE.to_le_bytes());
        pd[2..4].copy_from_slice(&(w as u16).to_le_bytes());
        pd[4..6].copy_from_slice(&(h as u16).to_le_bytes());
        pd[6..8].copy_from_slice(&(w as u16).to_le_bytes());
        pd[8] = 1;
        pd[9] = 8;
        self.hw.write_bytes(at, &pd);
        let d = &mut self.disp;
        if (d.w, d.h) != (w, h) || d.pix.is_empty() {
            d.w = w;
            d.h = h;
            d.pix = vec![0; w * h];
        }
        d.enabled = true;
        d.touch(Rect::new(0, 0, w as i32, h as i32));
        self.mouse.screen = Some((w as i32, h as i32));
        1
    }

    /// The device at a PDEVICE or BITMAP pointer.
    pub(crate) fn disp_dev(&mut self, p: Ptr) -> Option<Dev> {
        if p.null() {
            return None;
        }
        let l = self.far(p);
        let ty = self.hw.rd16(l);
        if ty != 0 {
            if !self.disp.enabled {
                return None;
            }
            return Some(Dev { screen: true, w: self.disp.w as i32, h: self.disp.h as i32, mono: false, stride: self.disp.w, scan_seg: 0, segs: Vec::new() });
        }
        let w = self.hw.rd16(l + 2) as i32;
        let h = self.hw.rd16(l + 4) as i32;
        let stride = self.hw.rd16(l + 6) as usize;
        let planes = self.hw.rd(l + 8);
        let bpp = self.hw.rd(l + 9);
        let bits = Ptr { off: self.hw.rd16(l + 10), sel: self.hw.rd16(l + 12) };
        let seg_index = self.hw.rd16(l + 22);
        let scan_seg = self.hw.rd16(l + 24) as usize;
        if bits.null() || planes != 1 || !(bpp == 1 || bpp == 8) {
            return None;
        }
        let (scan_seg, nsegs) = if seg_index != 0 && scan_seg != 0 { (scan_seg, (h as usize).div_ceil(scan_seg)) } else { (h.max(1) as usize, 1) };
        let mut segs = Vec::with_capacity(nsegs);
        for k in 0..nsegs {
            let sel = bits.sel.wrapping_add((k as u16).wrapping_mul(seg_index));
            let off = if k == 0 { bits.off } else { 0 };
            segs.push(self.dpmi_lin(sel, off as u32));
        }
        Some(Dev { screen: false, w, h, mono: bpp == 1, stride, scan_seg, segs })
    }

    pub(crate) fn disp_drawmode(&mut self, p: Ptr) -> DrawMode {
        if p.null() {
            return DrawMode::default();
        }
        let l = self.far(p);
        let w = |m: &mut Machine, o: u32| m.hw.rd16(l + o) as i16 as i32;
        DrawMode {
            rop2: w(self, 0).clamp(1, 16) as u8,
            bk_mode: w(self, 2) as u16,
            bk: self.hw.rd32(l + 4),
            text: self.hw.rd32(l + 8),
            break_extra: w(self, 14),
            break_err: w(self, 16),
            break_rem: w(self, 18),
            break_count: w(self, 20),
            char_extra: w(self, 22),
        }
    }

    /// COLORINFO(lpDestDev, dwColorIn, lpPColor): an RGB colour to a
    /// physical one, or (lpPColor NULL) a physical colour to RGB.
    fn disp_colorinfo(&mut self, a: &mut Args) -> u32 {
        let dev = a.p();
        let c = a.d();
        let out = a.p();
        let mono = self.disp_dev(dev).is_some_and(|d| d.mono);
        if out.null() {
            return if mono { if c & 0x100 != 0 { 0xffffff } else { 0 } } else { cref_of(c as u8) };
        }
        let p = phys(c);
        let l = self.far(out);
        self.hw.wr32(l, p);
        if mono { if p & 0x100 != 0 { 0xffffff } else { 0 } } else { cref_of(p as u8) }
    }

    /// SETCURSOR(lpCursorShape): the pointer shape, NULL to hide it.
    fn disp_setcursor(&mut self, a: &mut Args) -> u32 {
        let p = a.p();
        self.disp.pointer_gen = self.disp.pointer_gen.wrapping_add(1);
        if p.null() {
            self.disp.pointer = None;
            return 0;
        }
        let l = self.far(p);
        let hx = self.hw.rd16(l) as i16 as i32;
        let hy = self.hw.rd16(l + 2) as i16 as i32;
        let w = (self.hw.rd16(l + 4) as usize).min(64);
        let h = (self.hw.rd16(l + 6) as usize).min(64);
        let wb = (self.hw.rd16(l + 8) as usize).min(8);
        let mut and = vec![0u8; wb * h];
        let mut xor = vec![0u8; wb * h];
        self.hw.read_bytes(l + 12, &mut and);
        self.hw.read_bytes(l + 12 + (wb * h) as u32, &mut xor);
        self.disp.pointer = Some(Pointer { hot: (hx, hy), w, h, width_bytes: wb, and, xor });
        0
    }

    /// SAVESCREENBITMAP(lpRect, wCommand): 0 save, 1 restore, 2 discard.
    fn disp_savescreen(&mut self, a: &mut Args) -> u32 {
        let pr = a.p();
        let cmd = a.w();
        let l = self.far(pr);
        let r = Rect::new(self.hw.rd16(l) as i16 as i32, self.hw.rd16(l + 2) as i16 as i32, self.hw.rd16(l + 4) as i16 as i32, self.hw.rd16(l + 6) as i16 as i32);
        let d = &mut self.disp;
        let r = r.intersect(&Rect::new(0, 0, d.w as i32, d.h as i32));
        match cmd {
            0 => {
                if r.is_empty() {
                    return 0;
                }
                let mut v = Vec::with_capacity(r.area() as usize);
                for y in r.y0..r.y1 {
                    let o = y as usize * d.w;
                    v.extend_from_slice(&d.pix[o + r.x0 as usize..o + r.x1 as usize]);
                }
                d.saved = Some((r, v));
                1
            }
            1 => match d.saved.take() {
                Some((s, v)) if s == r => {
                    let wid = (r.x1 - r.x0) as usize;
                    for (k, y) in (r.y0..r.y1).enumerate() {
                        let o = y as usize * d.w;
                        d.pix[o + r.x0 as usize..o + r.x1 as usize].copy_from_slice(&v[k * wid..(k + 1) * wid]);
                    }
                    d.touch(r);
                    1
                }
                other => {
                    d.saved = other;
                    0
                }
            },
            _ => {
                d.saved = None;
                1
            }
        }
    }
}

/// GDIINFO for a w x h screen of 8-bit pixels, with VGA.DRV's
/// capabilities for what it shares: polylines (styled), scan lines,
/// clipping to rectangles, BitBlt, 64 KB+ bitmaps, SaveScreenBitmap,
/// device-independent bitmaps, 3.0 fonts, ExtTextOut widths; at 96 dpi.
fn gdiinfo(w: usize, h: usize) -> Vec<u8> {
    let (hs, vs) = ((w * 13 / 40) as i32, (h * 13 / 40) as i32);
    // HIMETRIC: 0.01 mm per unit, kept in 16 bits.
    let (mut mhx, mut mhy, mut mvx, mut mvy) = (hs * 100, vs * 100, w as i32, h as i32);
    while mhx > 32767 || mhy > 32767 {
        (mhx, mhy, mvx, mvy) = (mhx / 2, mhy / 2, mvx / 2, mvy / 2);
    }
    let v: &[i32] = &[
        0x030a, // dpVersion
        1,      // DT_RASDISPLAY
        hs,
        vs,
        w as i32,
        h as i32,
        8, // bits per pixel
        1, // planes
        -1,
        80,
        0,
        0,
        256, // colours
        PDEVICE_SIZE as i32,
        0,      // curves: none (GDI makes polylines)
        0x22,   // LC_POLYLINE | LC_STYLED
        0x08,   // PC_SCANLINE
        0x2204, // TC_CP_STROKE | TC_EA_DOUBLE | TC_RA_ABLE
        1,      // CP_RECTANGLE
        // RC_BITBLT, RC_BITMAP64, RC_GDI20_OUTPUT, RC_SAVEBITMAP,
        // RC_DI_BITMAP, RC_BIGFONT, RC_OP_DX_OUTPUT
        0x44d9,
        36, // aspect x, y, xy
        36,
        51,
        102, // style length
        hs * 10, // MM_LOMETRIC
        vs * 10,
        w as i32,
        -(h as i32),
        mhx, // MM_HIMETRIC
        mhy,
        mvx,
        -mvy,
        325, // MM_LOENGLISH
        325,
        254,
        -254,
        1625, // MM_HIENGLISH
        1625,
        127,
        -127,
        2340, // MM_TWIPS
        2340,
        127,
        -127,
        96, // logical pixels per inch
        96,
        4, // dpDCManage
    ];
    let mut g = Vec::with_capacity(110);
    for &x in v {
        g.extend_from_slice(&(x as i16).to_le_bytes());
    }
    g.resize(110, 0);
    g
}
