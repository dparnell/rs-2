//! The VGA (with the EGA and CGA modes it covers): registers, planar
//! memory at A0000h, the DAC, and rendering of graphics modes into an
//! 8-bit indexed picture. Text modes and the CGA graphics modes keep
//! their memory in ordinary RAM at B8000h (B0000h for mode 7), which the
//! renderer and the host read from there.

use alloc::vec;
use alloc::vec::Vec;

pub const PLANE: usize = 0x10000;

#[derive(Clone)]
pub struct Vga {
    pub planes: Vec<u8>,
    pub latch: [u8; 4],
    pub seq: [u8; 8],
    pub seq_idx: u8,
    pub gc: [u8; 9],
    pub gc_idx: u8,
    pub crtc: [u8; 32],
    pub crtc_idx: u8,
    pub attr: [u8; 21],
    pub attr_idx: u8,
    /// Port 3C0h: the next write is an index (false) or data (true).
    pub attr_flip: bool,
    pub misc: u8,
    pub dac: [[u8; 3]; 256],
    pub dac_read: u8,
    pub dac_write: u8,
    /// Component (0-2) of the entry being read or written.
    pub dac_comp: u8,
    pub dac_rcomp: u8,
    pub pel_mask: u8,
    /// The BIOS mode number last set.
    pub mode: u8,
    /// CGA colour select register (port 3D9h) and mode control (3D8h).
    pub cga_color: u8,
    pub cga_mode: u8,
    /// Something visible changed since the last [`Vga::take_dirty`].
    pub dirty: bool,
    /// The palette or the DAC changed.
    pub palette_dirty: bool,
}

impl Default for Vga {
    fn default() -> Self {
        Self::new()
    }
}

/// What the screen shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Text: columns, rows, memory base (B8000h or B0000h).
    Text { cols: u16, rows: u16, base: u32 },
    /// Graphics: pixels across and down (before double scanning).
    Graphics { width: u16, height: u16 },
}

impl Vga {
    pub fn new() -> Vga {
        let mut v = Vga {
            planes: vec![0; 4 * PLANE],
            latch: [0; 4],
            seq: [0; 8],
            seq_idx: 0,
            gc: [0; 9],
            gc_idx: 0,
            crtc: [0; 32],
            crtc_idx: 0,
            attr: [0; 21],
            attr_idx: 0,
            attr_flip: false,
            misc: 0,
            dac: [[0; 3]; 256],
            dac_read: 0,
            dac_write: 0,
            dac_comp: 0,
            dac_rcomp: 0,
            pel_mask: 0xff,
            mode: 3,
            cga_color: 0,
            cga_mode: 0,
            dirty: true,
            palette_dirty: true,
        };
        v.set_mode_regs(3);
        v.load_dac(3);
        v
    }

    pub fn take_dirty(&mut self) -> bool {
        core::mem::take(&mut self.dirty)
    }

    /// Graphics modes map planar memory at A0000h.
    pub fn planar(&self) -> bool {
        self.gc[6] & 1 != 0 && (self.gc[6] >> 2) & 3 <= 1
    }

    pub fn chain4(&self) -> bool {
        self.seq[4] & 8 != 0
    }

    // ---- Memory at A0000h-AFFFFh ----

    pub fn read(&mut self, off: u32) -> u8 {
        let off = off as usize & 0xffff;
        if self.chain4() {
            let o = off >> 2;
            for p in 0..4 {
                self.latch[p] = self.planes[p * PLANE + o];
            }
            return self.latch[off & 3];
        }
        for p in 0..4 {
            self.latch[p] = self.planes[p * PLANE + off];
        }
        if self.gc[5] & 8 == 0 {
            self.latch[(self.gc[4] & 3) as usize]
        } else {
            // Read mode 1: colour compare.
            let mut r = 0xff;
            for p in 0..4 {
                if self.gc[7] & (1 << p) != 0 {
                    let want = if self.gc[2] & (1 << p) != 0 { 0xff } else { 0 };
                    r &= !(self.latch[p] ^ want);
                }
            }
            r
        }
    }

    pub fn write(&mut self, off: u32, v: u8) {
        let off = off as usize & 0xffff;
        self.dirty = true;
        let mask = self.seq[2];
        if self.chain4() {
            let p = off & 3;
            if mask & (1 << p) != 0 {
                self.planes[p * PLANE + (off >> 2)] = v;
            }
            return;
        }
        let wm = self.gc[5] & 3;
        let rot = self.gc[3] & 7;
        let op = (self.gc[3] >> 3) & 3;
        let bitmask = self.gc[8];
        let rv = v.rotate_right(rot as u32);
        for p in 0..4 {
            if mask & (1 << p) == 0 {
                continue;
            }
            let latch = self.latch[p];
            let sr = if self.gc[0] & (1 << p) != 0 { 0xff } else { 0 };
            let r = match wm {
                0 => {
                    let d = if self.gc[1] & (1 << p) != 0 { sr } else { rv };
                    let d = Self::alu(op, d, latch);
                    (d & bitmask) | (latch & !bitmask)
                }
                1 => latch,
                2 => {
                    let d = if v & (1 << p) != 0 { 0xff } else { 0 };
                    let d = Self::alu(op, d, latch);
                    (d & bitmask) | (latch & !bitmask)
                }
                _ => {
                    let m = rv & bitmask;
                    let d = Self::alu(op, sr, latch);
                    (d & m) | (latch & !m)
                }
            };
            self.planes[p * PLANE + off] = r;
        }
    }

    fn alu(op: u8, d: u8, latch: u8) -> u8 {
        match op {
            0 => d,
            1 => d & latch,
            2 => d | latch,
            _ => d ^ latch,
        }
    }

    // ---- Ports 3B0h-3DFh ----

    pub fn io_read(&mut self, port: u16, retrace: (bool, bool)) -> u8 {
        match port {
            0x3c0 => self.attr_idx,
            0x3c1 => self.attr.get((self.attr_idx & 0x1f) as usize).copied().unwrap_or(0),
            0x3c2 => 0x10,
            0x3c4 => self.seq_idx,
            0x3c5 => self.seq.get(self.seq_idx as usize).copied().unwrap_or(0),
            0x3c6 => self.pel_mask,
            0x3c7 => 0,
            0x3c8 => self.dac_write,
            0x3c9 => {
                let v = self.dac[self.dac_read as usize][self.dac_rcomp as usize];
                self.dac_rcomp += 1;
                if self.dac_rcomp == 3 {
                    self.dac_rcomp = 0;
                    self.dac_read = self.dac_read.wrapping_add(1);
                }
                v
            }
            0x3cc => self.misc,
            0x3ce => self.gc_idx,
            0x3cf => self.gc.get(self.gc_idx as usize).copied().unwrap_or(0),
            0x3b4 | 0x3d4 => self.crtc_idx,
            0x3b5 | 0x3d5 => self.crtc.get(self.crtc_idx as usize).copied().unwrap_or(0),
            0x3ba | 0x3da => {
                self.attr_flip = false;
                let (vr, de) = retrace;
                (vr as u8) << 3 | (de as u8)
            }
            _ => 0xff,
        }
    }

    pub fn io_write(&mut self, port: u16, v: u8) {
        match port {
            0x3c0 => {
                if !self.attr_flip {
                    self.attr_idx = v;
                } else {
                    let i = (self.attr_idx & 0x1f) as usize;
                    if i < self.attr.len() {
                        self.attr[i] = v;
                        self.palette_dirty = true;
                        self.dirty = true;
                    }
                }
                self.attr_flip = !self.attr_flip;
            }
            0x3c2 => self.misc = v,
            0x3c4 => self.seq_idx = v & 7,
            0x3c5 => {
                if (self.seq_idx as usize) < self.seq.len() {
                    self.seq[self.seq_idx as usize] = v;
                    self.dirty = true;
                }
            }
            0x3c6 => {
                self.pel_mask = v;
                self.palette_dirty = true;
            }
            0x3c7 => {
                self.dac_read = v;
                self.dac_rcomp = 0;
            }
            0x3c8 => {
                self.dac_write = v;
                self.dac_comp = 0;
            }
            0x3c9 => {
                self.dac[self.dac_write as usize][self.dac_comp as usize] = v & 0x3f;
                self.dac_comp += 1;
                if self.dac_comp == 3 {
                    self.dac_comp = 0;
                    self.dac_write = self.dac_write.wrapping_add(1);
                }
                self.palette_dirty = true;
                self.dirty = true;
            }
            0x3ce => self.gc_idx = v & 0xf,
            0x3cf => {
                if (self.gc_idx as usize) < self.gc.len() {
                    self.gc[self.gc_idx as usize] = v;
                }
            }
            0x3b4 | 0x3d4 => self.crtc_idx = v & 0x1f,
            0x3b5 | 0x3d5 => {
                let i = self.crtc_idx as usize;
                // Registers 0-7 are write-protected by CR11 bit 7.
                if i < 8 && self.crtc[0x11] & 0x80 != 0 && i != 7 {
                    return;
                }
                if i == 7 && self.crtc[0x11] & 0x80 != 0 {
                    // Only the line compare bit stays writable.
                    self.crtc[7] = (self.crtc[7] & !0x10) | (v & 0x10);
                    return;
                }
                self.crtc[i] = v;
                self.dirty = true;
            }
            0x3d8 => {
                self.cga_mode = v;
                self.dirty = true;
            }
            0x3d9 => {
                self.cga_color = v;
                self.palette_dirty = true;
                self.dirty = true;
            }
            _ => {}
        }
    }

    // ---- Mode tables ----

    /// Program the registers for BIOS mode `mode`.
    pub fn set_mode_regs(&mut self, mode: u8) {
        let t = mode_table(mode);
        self.misc = t.misc;
        self.seq[..5].copy_from_slice(&t.seq);
        self.crtc[..25].copy_from_slice(&t.crtc);
        self.gc.copy_from_slice(&t.gc);
        self.attr.copy_from_slice(&t.attr);
        self.mode = mode;
        self.attr_flip = false;
        self.dirty = true;
        self.palette_dirty = true;
        self.cga_color = if mode == 6 { 0x3f } else { 0x30 };
        self.cga_mode = match mode {
            0 => 0x2c,
            1 => 0x28,
            2 => 0x2d,
            3 => 0x29,
            4 => 0x2a,
            5 => 0x2e,
            6 => 0x1e,
            _ => 0x29,
        };
    }

    /// Load the DAC with the BIOS default palette for `mode`.
    pub fn load_dac(&mut self, mode: u8) {
        if mode == 0x13 {
            self.dac = default_256();
        } else {
            for i in 0..64 {
                let c = |hi: usize, lo: usize| ((i >> hi) & 1) as u8 * 42 + ((i >> lo) & 1) as u8 * 21;
                self.dac[i] = [c(2, 5), c(1, 4), c(0, 3)];
            }
            if mode == 7 || mode == 0xf {
                for i in 0..64 {
                    let g = [0u8, 42, 63, 63][(i >> 3 & 1) << 1 | (i >> 4 & 1)];
                    let g = if i & 0x18 == 0 { 0 } else { g };
                    self.dac[i] = [g, g, g];
                }
            }
            for i in 64..256 {
                self.dac[i] = [0, 0, 0];
            }
        }
        self.palette_dirty = true;
    }

    // ---- What is shown ----

    pub fn graphics(&self) -> bool {
        self.attr[0x10] & 1 != 0
    }

    /// Scan lines displayed (vertical display end + 1).
    pub fn scanlines(&self) -> u32 {
        let o = self.crtc[7] as u32;
        (self.crtc[0x12] as u32 | (o & 2) << 7 | (o & 0x40) << 3) + 1
    }

    /// Scan lines per character row / pixel row.
    fn row_height(&self) -> u32 {
        let msl = (self.crtc[9] & 0x1f) as u32 + 1;
        if self.crtc[9] & 0x80 != 0 { msl * 2 } else { msl }
    }

    pub fn kind(&self) -> Kind {
        let cols = self.crtc[1] as u32 + 1;
        if !self.graphics() {
            let rows = (self.scanlines() / self.row_height()).max(1);
            let base = if self.misc & 1 == 0 || self.mode == 7 { 0xb0000 } else { 0xb8000 };
            return Kind::Text { cols: cols as u16, rows: rows as u16, base };
        }
        let dots = cols * if self.seq[1] & 1 != 0 { 8 } else { 9 };
        let mut width = if self.seq[1] & 8 != 0 { dots / 2 } else { dots };
        if self.attr[0x10] & 0x40 != 0 {
            width /= 2;
        }
        let height = self.scanlines() / self.row_height().min(if self.is_cga() { 2 } else { 32 }).max(1);
        Kind::Graphics { width: width as u16, height: height as u16 }
    }

    /// CGA graphics modes 4-6 (memory at B8000h, interlaced).
    pub fn is_cga(&self) -> bool {
        self.graphics() && self.crtc[0x17] & 1 == 0
    }

    /// Text: the first cell shown (CRTC start address, in cells).
    pub fn text_start(&self) -> u32 {
        (self.crtc[0x0c] as u32) << 8 | self.crtc[0x0d] as u32
    }

    /// Text: the cursor cell offset and whether it is shown, and its
    /// first and last scan lines.
    pub fn cursor(&self) -> (u32, bool, u8, u8) {
        let pos = (self.crtc[0x0e] as u32) << 8 | self.crtc[0x0f] as u32;
        let start = self.crtc[0x0a];
        let end = self.crtc[0x0b] & 0x1f;
        (pos, start & 0x20 == 0, start & 0x1f, end)
    }

    /// Attribute bit 7: blinking (true) or a bright background.
    pub fn blink(&self) -> bool {
        self.attr[0x10] & 8 != 0
    }

    /// The RGB (8 bits each, 0xRRGGBB) of DAC entry `i`.
    pub fn rgb(&self, i: u8) -> u32 {
        let i = i & self.pel_mask;
        let [r, g, b] = self.dac[i as usize];
        let x = |c: u8| ((c as u32) << 2) | ((c as u32) >> 4);
        x(r) << 16 | x(g) << 8 | x(b)
    }

    /// The DAC index the attribute controller gives colour `c` (0-15).
    pub fn attr_color(&self, c: u8) -> u8 {
        let p = self.attr[(c & 0xf) as usize];
        let sel = self.attr[0x14];
        if self.attr[0x10] & 0x80 != 0 {
            (p & 0x0f) | (sel & 0x0f) << 4
        } else {
            (p & 0x3f) | (sel & 0x0c) << 4
        }
    }

    /// The 16 text colours as RGB.
    pub fn text_palette(&self) -> [u32; 16] {
        let mut p = [0; 16];
        for (i, c) in p.iter_mut().enumerate() {
            *c = self.rgb(self.attr_color(i as u8));
        }
        p
    }

    /// The palette of a graphics picture from [`Vga::render`]: 256 RGB
    /// entries for the indices it uses.
    pub fn palette(&self) -> [u32; 256] {
        let mut p = [0; 256];
        if self.attr[0x10] & 0x40 != 0 {
            for (i, c) in p.iter_mut().enumerate() {
                *c = self.rgb(i as u8);
            }
        } else {
            for (i, c) in p.iter_mut().enumerate().take(16) {
                *c = self.rgb(self.attr_color(i as u8));
            }
        }
        p
    }

    /// Render a graphics mode into `out` (width * height indices into
    /// [`Vga::palette`]); `ram` is the machine's memory (for the CGA
    /// modes). Returns (width, height).
    pub fn render(&self, ram: &[u8], out: &mut Vec<u8>) -> (usize, usize) {
        let Kind::Graphics { width, height } = self.kind() else { return (0, 0) };
        let (w, h) = (width as usize, height as usize);
        out.clear();
        out.resize(w * h, 0);
        let start = self.text_start() as usize;
        // Bytes per row in each plane (planes are addressed as chain-4
        // leaves them: pixel x of a 256-colour row is at x / 4).
        let offset = self.crtc[0x13] as usize * 2;
        if self.is_cga() {
            let bits2 = self.cga_mode & 0x10 == 0;
            let base = 0xb8000;
            // CGA palettes: 0 green/red/brown, 1 cyan/magenta/white.
            let pal1 = self.cga_color & 0x20 != 0 || self.mode == 5;
            let bright = if self.cga_color & 0x10 != 0 { 8 } else { 0 };
            let bg = self.cga_color & 0x0f;
            for y in 0..h {
                let row = base + (y & 1) * 0x2000 + (y >> 1) * 80;
                for x in 0..w {
                    let c = if bits2 {
                        let b = ram.get(row + x / 4).copied().unwrap_or(0);
                        let v = (b >> (6 - (x & 3) * 2)) & 3;
                        if v == 0 {
                            bg
                        } else if pal1 {
                            [0, 3, 5, 7][v as usize] | bright
                        } else {
                            [0, 2, 4, 6][v as usize] | bright
                        }
                    } else {
                        let b = ram.get(row + x / 8).copied().unwrap_or(0);
                        if (b >> (7 - (x & 7))) & 1 != 0 { self.cga_color & 0x0f } else { 0 }
                    };
                    out[y * w + x] = cga_rgbi_to_ega(c);
                }
            }
            return (w, h);
        }
        let lc = self.line_compare(h);
        if self.attr[0x10] & 0x40 != 0 {
            // 256 colours: chain-4 (mode 13h) or unchained (mode X); both
            // keep pixel x of a row in plane x & 3 at address x / 4.
            let pan = (self.attr[0x13] & 3) as usize;
            for y in 0..h {
                let a = if y >= lc { (y - lc) * offset } else { start + y * offset };
                for x in 0..w {
                    let px = x + pan;
                    let o = (a + px / 4) & 0xffff;
                    out[y * w + x] = self.planes[(px & 3) * PLANE + o];
                }
            }
            return (w, h);
        }
        // 16 colours (or 2/4) from four bit planes.
        let enable = self.attr[0x12] & 0xf;
        let pan = (self.attr[0x13] & 7) as usize;
        for y in 0..h {
            let a = if y >= lc { (y - lc) * offset } else { start + y * offset };
            for x in 0..w {
                let px = x + pan;
                let o = (a + px / 8) & 0xffff;
                let bit = 7 - (px & 7);
                let mut c = 0;
                for p in 0..4 {
                    c |= ((self.planes[p * PLANE + o] >> bit) & 1) << p;
                }
                out[y * w + x] = c & enable;
            }
        }
        (w, h)
    }

    /// The pixel row where the line compare register restarts at address 0.
    fn line_compare(&self, h: usize) -> usize {
        let o = self.crtc[7] as usize;
        let lc = self.crtc[0x18] as usize | (o & 0x10) << 4 | (self.crtc[9] as usize & 0x40) << 3;
        let rh = self.row_height().min(if self.is_cga() { 2 } else { 32 }).max(1) as usize;
        let row = (lc + 1) / rh;
        if row >= h { usize::MAX } else { row }
    }
}

/// CGA RGBI colour to the EGA palette index the attribute controller
/// holds for it in modes 4-6 (identity for 0-15 there).
fn cga_rgbi_to_ega(c: u8) -> u8 {
    c & 0x0f
}

/// The VGA BIOS's default 256-colour palette.
fn default_256() -> [[u8; 3]; 256] {
    let mut d = [[0u8; 3]; 256];
    let ega = [0u8, 42, 0, 42, 0, 42, 0, 42, 21, 63, 21, 63, 21, 63, 21, 63];
    for (i, e) in d.iter_mut().enumerate().take(16) {
        let r = if i & 4 != 0 { 42 } else { 0 } + if i & 8 != 0 { 21 } else { 0 };
        let g = if i & 2 != 0 { 42 } else { 0 } + if i & 8 != 0 { 21 } else { 0 };
        let b = ega[i];
        let g = if i == 6 { 21 } else { g };
        *e = [r, g, b];
    }
    let grays = [0u8, 5, 8, 11, 14, 17, 20, 24, 28, 32, 36, 40, 45, 50, 56, 63];
    for (i, g) in grays.iter().enumerate() {
        d[16 + i] = [*g, *g, *g];
    }
    // Hue circles: three intensities, three saturations, 24 hues each.
    let mut i = 32;
    for (hi, lo, mid) in [(63u8, 0u8, 0u8), (63, 31, 31), (63, 45, 45), (28, 0, 0), (28, 14, 14), (28, 20, 20), (16, 0, 0), (16, 8, 8), (16, 11, 11)] {
        let _ = mid;
        let steps = [lo, lo + (hi - lo) / 4, lo + (hi - lo) / 2, lo + 3 * (hi - lo) / 4, hi];
        // 24 hues: blue→magenta→red→yellow→green→cyan→blue.
        let mut hue = Vec::new();
        for s in steps {
            hue.push([s, lo, hi]);
        }
        for s in steps[1..].iter().rev() {
            hue.push([hi, lo, *s]);
        }
        for s in &steps[1..] {
            hue.push([hi, *s, lo]);
        }
        for s in steps[1..].iter().rev() {
            hue.push([*s, hi, lo]);
        }
        for s in &steps[1..] {
            hue.push([lo, hi, *s]);
        }
        for s in steps[1..4].iter().rev() {
            hue.push([lo, *s, hi]);
        }
        for c in hue.iter().take(24) {
            if i < 248 {
                d[i] = *c;
                i += 1;
            }
        }
    }
    d
}

struct ModeRegs {
    misc: u8,
    seq: [u8; 5],
    crtc: [u8; 25],
    gc: [u8; 9],
    attr: [u8; 21],
}

const TEXT_ATTR: [u8; 21] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x14, 0x07, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x0c, 0x00, 0x0f, 0x08, 0x00];
const EGA_ATTR: [u8; 21] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x01, 0x00, 0x0f, 0x00, 0x00];
const VGA16_ATTR: [u8; 21] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x14, 0x07, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x01, 0x00, 0x0f, 0x00, 0x00];
const TEXT_GC: [u8; 9] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x0e, 0x00, 0xff];
const PLANAR_GC: [u8; 9] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x0f, 0xff];

fn mode_table(mode: u8) -> ModeRegs {
    match mode {
        0 | 1 => ModeRegs {
            misc: 0x67,
            seq: [0x03, 0x08, 0x03, 0x00, 0x02],
            crtc: [0x2d, 0x27, 0x28, 0x90, 0x2b, 0xa0, 0xbf, 0x1f, 0x00, 0x4f, 0x0d, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x14, 0x1f, 0x96, 0xb9, 0xa3, 0xff],
            gc: TEXT_GC,
            attr: TEXT_ATTR,
        },
        4 | 5 => ModeRegs {
            misc: 0x63,
            seq: [0x03, 0x09, 0x03, 0x00, 0x02],
            crtc: [0x2d, 0x27, 0x28, 0x90, 0x2b, 0x80, 0xbf, 0x1f, 0x00, 0xc1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x14, 0x00, 0x96, 0xb9, 0xa2, 0xff],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x0f, 0x00, 0xff],
            attr: [0x00, 0x13, 0x15, 0x17, 0x02, 0x04, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x01, 0x00, 0x03, 0x00, 0x00],
        },
        6 => ModeRegs {
            misc: 0x63,
            seq: [0x03, 0x01, 0x01, 0x00, 0x06],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x1f, 0x00, 0xc1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x28, 0x00, 0x96, 0xb9, 0xc2, 0xff],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x00, 0xff],
            attr: [0x00, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x01, 0x00, 0x01, 0x00, 0x00],
        },
        7 => ModeRegs {
            misc: 0x66,
            seq: [0x03, 0x00, 0x03, 0x00, 0x02],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x55, 0x81, 0xbf, 0x1f, 0x00, 0x4f, 0x0d, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x28, 0x0f, 0x96, 0xb9, 0xa3, 0xff],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x0a, 0x00, 0xff],
            attr: [0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x10, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x0e, 0x00, 0x0f, 0x08, 0x00],
        },
        0x0d => ModeRegs {
            misc: 0x63,
            seq: [0x03, 0x09, 0x0f, 0x00, 0x06],
            crtc: [0x2d, 0x27, 0x28, 0x90, 0x2b, 0x80, 0xbf, 0x1f, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x14, 0x00, 0x96, 0xb9, 0xe3, 0xff],
            gc: PLANAR_GC,
            attr: EGA_ATTR,
        },
        0x0e => ModeRegs {
            misc: 0x63,
            seq: [0x03, 0x01, 0x0f, 0x00, 0x06],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x1f, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x28, 0x00, 0x96, 0xb9, 0xe3, 0xff],
            gc: PLANAR_GC,
            attr: EGA_ATTR,
        },
        0x0f => ModeRegs {
            misc: 0xa2,
            seq: [0x03, 0x01, 0x0f, 0x00, 0x06],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x1f, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x83, 0x85, 0x5d, 0x28, 0x0f, 0x63, 0xba, 0xe3, 0xff],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x05, 0xff],
            attr: [0x00, 0x08, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x0b, 0x00, 0x05, 0x00, 0x00],
        },
        0x10 => ModeRegs {
            misc: 0xa3,
            seq: [0x03, 0x01, 0x0f, 0x00, 0x06],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x1f, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x83, 0x85, 0x5d, 0x28, 0x0f, 0x63, 0xba, 0xe3, 0xff],
            gc: PLANAR_GC,
            attr: VGA16_ATTR,
        },
        0x11 => ModeRegs {
            misc: 0xe3,
            seq: [0x03, 0x01, 0x0f, 0x00, 0x06],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0x0b, 0x3e, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xea, 0x8c, 0xdf, 0x28, 0x00, 0xe7, 0x04, 0xc3, 0xff],
            gc: PLANAR_GC,
            attr: [0x00, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x01, 0x00, 0x0f, 0x00, 0x00],
        },
        0x12 => ModeRegs {
            misc: 0xe3,
            seq: [0x03, 0x01, 0x0f, 0x00, 0x06],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0x0b, 0x3e, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xea, 0x8c, 0xdf, 0x28, 0x00, 0xe7, 0x04, 0xe3, 0xff],
            gc: PLANAR_GC,
            attr: VGA16_ATTR,
        },
        0x13 => ModeRegs {
            misc: 0x63,
            seq: [0x03, 0x01, 0x0f, 0x00, 0x0e],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x1f, 0x00, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x28, 0x40, 0x96, 0xb9, 0xa3, 0xff],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x05, 0x0f, 0xff],
            attr: [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x41, 0x00, 0x0f, 0x00, 0x00],
        },
        // Modes 2 and 3 (and anything unknown): 80x25 colour text.
        _ => ModeRegs {
            misc: 0x67,
            seq: [0x03, 0x00, 0x03, 0x00, 0x02],
            crtc: [0x5f, 0x4f, 0x50, 0x82, 0x55, 0x81, 0xbf, 0x1f, 0x00, 0x4f, 0x0d, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x9c, 0x8e, 0x8f, 0x28, 0x1f, 0x96, 0xb9, 0xa3, 0xff],
            gc: TEXT_GC,
            attr: TEXT_ATTR,
        },
    }
}
