//! OS/2's icon, pointer and bitmap files (pmbitmap.h), read and written
//! without PM: a figure is a list of forms, one per device form of a
//! bitmap array (`BA`) or the single image of a plain file.
//!
//! - File headers: BITMAPFILEHEADER (14 bytes: usType, cbSize, xHotspot,
//!   yHotspot, offBits) followed by a BITMAPINFOHEADER (OS/2 1.x, cbFix
//!   12: 16-bit cx, cy, planes, bit count; RGB colour entries of 3
//!   bytes) or a BITMAPINFOHEADER2 (OS/2 2.0, cbFix 16..64: 32-bit cx and
//!   cy, compression, colours used...; RGB2 entries of 4 bytes).
//! - Types: `BM` bitmap, `IC`/`PT` black and white icon/pointer (one
//!   1 bpp bitmap of twice the height: the XOR mask in the bottom half,
//!   the AND mask in the top half), `CI`/`CP` colour icon/pointer (that
//!   mask bitmap followed by a second header for the colour bitmap),
//!   `BA` an array of them, each entry a BITMAPARRAYFILEHEADER (usType,
//!   cbSize, offNext from the start of the file, cxDisplay, cyDisplay:
//!   0 for device independent forms) around one of the others.
//! - Pels: rows from the bottom, each padded to 4 bytes, 1, 4, 8 or 24
//!   bits (blue, green, red); 2.0 files may be compressed (RLE8, RLE4).
//! - Masks: AND 0 XOR 0 is the colour (black in a black and white
//!   figure), AND 0 XOR 1 white, AND 1 XOR 0 the screen showing through,
//!   AND 1 XOR 1 the screen inverted.
//!
//! The editor keeps every pel as an RGB value (0xRRGGBB) or SCREEN or
//! INVERSE; writing maps colours to the form's palette.

use alloc::vec::Vec;

/// A pel that shows the screen.
pub const SCREEN: u32 = 0x0100_0000;
/// A pel that shows the screen inverted.
pub const INVERSE: u32 = 0x0200_0000;

/// The largest form the editor takes (pels per side).
pub const MAX_SIDE: u32 = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Icon,
    Pointer,
    Bitmap,
}

/// The layouts Save as offers (OS/2's Save Options).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// One image, 1.x headers.
    V11,
    /// A bitmap array, 1.x headers.
    V12,
    /// A bitmap array, 2.0 headers.
    V20,
    /// A bitmap array, 2.0 headers, 4 and 8 bit pels compressed.
    V20Packed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub cx: u32,
    pub cy: u32,
    /// Colour bits: 1, 4, 8 or 24.
    pub bpp: u16,
    /// RGB entries (2, 16 or 256; none for 24 bits).
    pub palette: Vec<u32>,
    /// Rows from the top, RGB or SCREEN or INVERSE.
    pub pels: Vec<u32>,
    /// The hot spot, from the bottom left.
    pub hot: (i32, i32),
    /// The display the form is for (0, 0: independent).
    pub display: (u16, u16),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    pub kind: Kind,
    pub forms: Vec<Form>,
    /// The layout the file was read in.
    pub format: Format,
}

/// The default palettes: black and white, the 16 VGA colours in OS/2's
/// order, and 256 colours (the 16, a 6x6x6 cube and greys).
pub fn default_palette(bpp: u16) -> Vec<u32> {
    match bpp {
        1 => alloc::vec![0x000000, 0xffffff],
        4 => VGA.to_vec(),
        8 => {
            let mut v = VGA.to_vec();
            for r in 0..6u32 {
                for g in 0..6u32 {
                    for b in 0..6u32 {
                        v.push(r * 51 << 16 | g * 51 << 8 | b * 51);
                    }
                }
            }
            let mut i = 0;
            while v.len() < 256 {
                let g = 8 + i * 10;
                v.push(g << 16 | g << 8 | g);
                i += 1;
            }
            v
        }
        _ => Vec::new(),
    }
}

/// The 16 colours of VGA, in the order OS/2's default palette has them.
pub const VGA: [u32; 16] = [
    0x000000, 0x000080, 0x008000, 0x008080, 0x800000, 0x800080, 0x808000, 0x808080, 0xcccccc, 0x0000ff, 0x00ff00, 0x00ffff, 0xff0000, 0xff00ff, 0xffff00, 0xffffff,
];

impl Form {
    /// A form filled with one value.
    pub fn new(cx: u32, cy: u32, bpp: u16, fill: u32) -> Form {
        Form { cx, cy, bpp, palette: default_palette(bpp), pels: alloc::vec![fill; (cx * cy) as usize], hot: (cx as i32 / 2, cy as i32 / 2), display: (0, 0) }
    }

    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.cx as i32 || y >= self.cy as i32 {
            return 0;
        }
        self.pels[(y as u32 * self.cx + x as u32) as usize]
    }

    pub fn set(&mut self, x: i32, y: i32, v: u32) {
        if x < 0 || y < 0 || x >= self.cx as i32 || y >= self.cy as i32 {
            return;
        }
        self.pels[(y as u32 * self.cx + x as u32) as usize] = v;
    }

    /// The palette index nearest to an RGB value.
    pub fn nearest(&self, rgb: u32) -> usize {
        nearest(&self.palette, rgb)
    }

    /// The same picture at another size (nearest pel).
    pub fn scaled(&self, cx: u32, cy: u32) -> Vec<u32> {
        let mut v = Vec::with_capacity((cx * cy) as usize);
        for y in 0..cy {
            for x in 0..cx {
                let sx = (x * self.cx / cx.max(1)) as i32;
                let sy = (y * self.cy / cy.max(1)) as i32;
                v.push(self.get(sx, sy));
            }
        }
        v
    }
}

pub fn nearest(pal: &[u32], rgb: u32) -> usize {
    let (r, g, b) = ((rgb >> 16 & 255) as i32, (rgb >> 8 & 255) as i32, (rgb & 255) as i32);
    let mut best = (i32::MAX, 0);
    for (i, &p) in pal.iter().enumerate() {
        let (pr, pg, pb) = ((p >> 16 & 255) as i32, (p >> 8 & 255) as i32, (p & 255) as i32);
        let d = (r - pr) * (r - pr) * 3 + (g - pg) * (g - pg) * 4 + (b - pb) * (b - pb) * 2;
        if d < best.0 {
            best = (d, i);
            if d == 0 {
                break;
            }
        }
    }
    best.1
}

fn r16(b: &[u8], o: usize) -> Result<u16, &'static str> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or("truncated file")
}

fn r32(b: &[u8], o: usize) -> Result<u32, &'static str> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or("truncated file")
}

/// One bitmap's header as read: size, bits, colours and where they are.
struct Info {
    cx: u32,
    cy: u32,
    bpp: u16,
    compression: u32,
    palette: Vec<u32>,
    off_bits: u32,
    hot: (i32, i32),
    v2: bool,
    /// The offset after the header and its colour table.
    end: usize,
}

/// Read a BITMAPFILEHEADER(2) with its info header and colour table at
/// `p`.
fn read_header(b: &[u8], p: usize) -> Result<Info, &'static str> {
    let hx = r16(b, p + 6)? as i16 as i32;
    let hy = r16(b, p + 8)? as i16 as i32;
    let off_bits = r32(b, p + 10)?;
    let ih = p + 14;
    let fix = r32(b, ih)?;
    let (cx, cy, bpp, compression, used, v2);
    if fix == 12 {
        cx = r16(b, ih + 4)? as u32;
        cy = r16(b, ih + 6)? as u32;
        bpp = r16(b, ih + 10)?;
        compression = 0;
        used = 0;
        v2 = false;
    } else if (16..=64).contains(&fix) {
        cx = r32(b, ih + 4)?;
        cy = r32(b, ih + 8)?;
        bpp = r16(b, ih + 14)?;
        compression = if fix >= 20 { r32(b, ih + 16)? } else { 0 };
        used = if fix >= 36 { r32(b, ih + 32)? } else { 0 };
        v2 = true;
    } else {
        return Err("unknown bitmap header");
    }
    if cx == 0 || cy == 0 || cx > MAX_SIDE || cy > MAX_SIDE * 2 {
        return Err("bitmap dimensions out of range");
    }
    if !matches!(bpp, 1 | 4 | 8 | 24) {
        return Err("unsupported number of colours");
    }
    let entry = if v2 { 4 } else { 3 };
    let n = if bpp > 8 { 0 } else if used != 0 && used < (1 << bpp) { used as usize } else { 1usize << bpp };
    let ct = ih + fix as usize;
    let mut palette = Vec::with_capacity(n);
    for i in 0..n {
        let o = ct + i * entry;
        let s = b.get(o..o + 3).ok_or("truncated colour table")?;
        palette.push((s[2] as u32) << 16 | (s[1] as u32) << 8 | s[0] as u32);
    }
    Ok(Info { cx, cy, bpp, compression, palette, off_bits, hot: (hx, hy), v2, end: ct + n * entry })
}

fn stride(cx: u32, bpp: u16) -> usize {
    ((cx as usize * bpp as usize + 31) / 32) * 4
}

/// The pels of a bitmap as values (palette indexes, or RGB for 24 bits),
/// rows from the bottom.
fn read_bits(b: &[u8], h: &Info) -> Result<Vec<u32>, &'static str> {
    let (cx, cy) = (h.cx as usize, h.cy as usize);
    let off = h.off_bits as usize;
    let mut out = alloc::vec![0u32; cx * cy];
    match h.compression {
        0 => {
            let st = stride(h.cx, h.bpp);
            if off + st * cy > b.len() {
                return Err("truncated bitmap bits");
            }
            for y in 0..cy {
                let row = &b[off + y * st..off + (y + 1) * st];
                for x in 0..cx {
                    out[y * cx + x] = match h.bpp {
                        1 => (row[x / 8] >> (7 - x % 8) & 1) as u32,
                        4 => (if x % 2 == 0 { row[x / 2] >> 4 } else { row[x / 2] & 15 }) as u32,
                        8 => row[x] as u32,
                        _ => (row[x * 3 + 2] as u32) << 16 | (row[x * 3 + 1] as u32) << 8 | row[x * 3] as u32,
                    };
                }
            }
        }
        // BCA_RLE8, BCA_RLE4: runs and escapes (0 0 end of line, 0 1 end,
        // 0 2 dx dy move, 0 n n pels as they are, padded to a word).
        1 | 2 if h.bpp == 8 || h.bpp == 4 => {
            let four = h.compression == 2;
            let (mut x, mut y, mut i) = (0usize, 0usize, off);
            let mut put = |x: &mut usize, y: usize, v: u32| {
                if *x < cx && y < cy {
                    out[y * cx + *x] = v;
                }
                *x += 1;
            };
            loop {
                let (Some(&n), Some(&v)) = (b.get(i), b.get(i + 1)) else { break };
                i += 2;
                if n > 0 {
                    for k in 0..n as usize {
                        let p = if four { if k % 2 == 0 { v >> 4 } else { v & 15 } } else { v };
                        put(&mut x, y, p as u32);
                    }
                    continue;
                }
                match v {
                    0 => {
                        x = 0;
                        y += 1;
                    }
                    1 => break,
                    2 => {
                        x += *b.get(i).unwrap_or(&0) as usize;
                        y += *b.get(i + 1).unwrap_or(&0) as usize;
                        i += 2;
                    }
                    n => {
                        let n = n as usize;
                        let bytes = if four { n.div_ceil(2) } else { n };
                        for k in 0..n {
                            let byte = *b.get(i + if four { k / 2 } else { k }).unwrap_or(&0);
                            let p = if four { if k % 2 == 0 { byte >> 4 } else { byte & 15 } } else { byte };
                            put(&mut x, y, p as u32);
                        }
                        i += bytes + bytes % 2;
                    }
                }
                if y >= cy {
                    break;
                }
            }
        }
        _ => return Err("compressed in a way the editor cannot read"),
    }
    Ok(out)
}

/// One image (BM, IC, PT, CI or CP) at `p`: its kind, form, whether it
/// had 2.0 headers and whether it was compressed.
fn read_image(b: &[u8], p: usize, display: (u16, u16)) -> Result<(Kind, Form, bool, bool), &'static str> {
    let ty = b.get(p..p + 2).ok_or("truncated file")?;
    let kind = match ty {
        b"BM" => Kind::Bitmap,
        b"IC" | b"CI" => Kind::Icon,
        b"PT" | b"CP" => Kind::Pointer,
        _ => return Err("not an OS/2 icon, pointer or bitmap"),
    };
    let h = read_header(b, p)?;
    if kind == Kind::Bitmap {
        let v = read_bits(b, &h)?;
        let (cx, cy) = (h.cx as usize, h.cy as usize);
        let mut pels = alloc::vec![0u32; cx * cy];
        for y in 0..cy {
            for x in 0..cx {
                let s = v[(cy - 1 - y) * cx + x];
                pels[y * cx + x] = if h.bpp > 8 { s } else { *h.palette.get(s as usize).unwrap_or(&0) };
            }
        }
        let palette = if h.bpp > 8 { Vec::new() } else { full_palette(&h.palette, h.bpp) };
        return Ok((kind, Form { cx: h.cx, cy: h.cy, bpp: h.bpp, palette, pels, hot: h.hot, display }, h.v2, h.compression != 0));
    }
    // The mask bitmap: XOR in the bottom half, AND in the top half.
    if h.bpp != 1 || h.cy % 2 != 0 {
        return Err("bad icon mask");
    }
    let masks = read_bits(b, &h)?;
    let (cx, cy) = (h.cx as usize, h.cy as usize / 2);
    let color = if ty == b"CI" || ty == b"CP" {
        let c = read_header(b, h.end)?;
        if c.cx as usize != cx || c.cy as usize != cy {
            return Err("icon colour bitmap does not match its mask");
        }
        let bits = read_bits(b, &c)?;
        Some((c, bits))
    } else {
        None
    };
    let mut pels = alloc::vec![0u32; cx * cy];
    for y in 0..cy {
        let r = cy - 1 - y;
        for x in 0..cx {
            let xor = masks[r * cx + x] != 0;
            let and = masks[(r + cy) * cx + x] != 0;
            pels[y * cx + x] = match (and, xor, &color) {
                (true, false, _) => SCREEN,
                (true, true, _) => INVERSE,
                (false, _, Some((c, bits))) => {
                    let s = bits[r * cx + x];
                    if c.bpp > 8 { s } else { *c.palette.get(s as usize).unwrap_or(&0) }
                }
                (false, xor, None) => {
                    if xor {
                        0xffffff
                    } else {
                        0
                    }
                }
            };
        }
    }
    let (bpp, palette) = match &color {
        Some((c, _)) => (c.bpp, if c.bpp > 8 { Vec::new() } else { full_palette(&c.palette, c.bpp) }),
        None => (1, default_palette(1)),
    };
    let packed = h.compression != 0 || color.as_ref().is_some_and(|(c, _)| c.compression != 0);
    Ok((kind, Form { cx: cx as u32, cy: cy as u32, bpp, palette, pels, hot: h.hot, display }, h.v2, packed))
}

/// A palette filled up to 2^bpp entries (files may list fewer).
fn full_palette(p: &[u32], bpp: u16) -> Vec<u32> {
    let mut v = p.to_vec();
    let d = default_palette(bpp);
    while v.len() < d.len() {
        v.push(d[v.len()]);
    }
    v
}

/// Read a file.
pub fn read(b: &[u8]) -> Result<Figure, &'static str> {
    if b.len() < 26 {
        return Err("too short for an image file");
    }
    if &b[0..2] != b"BA" {
        let (kind, form, v2, packed) = read_image(b, 0, (0, 0))?;
        let format = if packed { Format::V20Packed } else if v2 { Format::V20 } else if kind == Kind::Bitmap { Format::V12 } else { Format::V11 };
        return Ok(Figure { kind, forms: alloc::vec![form], format });
    }
    let mut forms = Vec::new();
    let mut kind = None;
    let mut any_v2 = false;
    let mut packed = false;
    let mut o = 0usize;
    for _ in 0..64 {
        if b.get(o..o + 2) != Some(b"BA") {
            return Err("bad bitmap array");
        }
        let next = r32(b, o + 6)? as usize;
        let display = (r16(b, o + 10)?, r16(b, o + 12)?);
        let (k, f, v2, p) = read_image(b, o + 14, display)?;
        any_v2 |= v2;
        packed |= p;
        if kind.is_none() {
            kind = Some(k);
        }
        forms.push(f);
        if next == 0 || next <= o {
            break;
        }
        o = next;
    }
    let format = if packed { Format::V20Packed } else if any_v2 { Format::V20 } else { Format::V12 };
    Ok(Figure { kind: kind.unwrap_or(Kind::Bitmap), forms, format })
}

/// A bitmap's bits to be written: (header fields, colour table, bits).
struct Out {
    cx: u32,
    cy: u32,
    bpp: u16,
    palette: Vec<u32>,
    bits: Vec<u8>,
    compression: u32,
}

/// Pack values (palette indexes or RGB; rows from the bottom).
fn pack(v: &[u32], cx: u32, cy: u32, bpp: u16) -> Vec<u8> {
    let st = stride(cx, bpp);
    let mut out = alloc::vec![0u8; st * cy as usize];
    for y in 0..cy as usize {
        let row = &mut out[y * st..(y + 1) * st];
        for x in 0..cx as usize {
            let p = v[y * cx as usize + x];
            match bpp {
                1 => row[x / 8] |= ((p & 1) as u8) << (7 - x % 8),
                4 => row[x / 2] |= ((p & 15) as u8) << if x % 2 == 0 { 4 } else { 0 },
                8 => row[x] = p as u8,
                _ => {
                    row[x * 3] = p as u8;
                    row[x * 3 + 1] = (p >> 8) as u8;
                    row[x * 3 + 2] = (p >> 16) as u8;
                }
            }
        }
    }
    out
}

/// RLE8 or RLE4 (runs only; a line ends with 0 0, the bitmap with 0 1).
fn rle(v: &[u32], cx: u32, cy: u32, four: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..cy as usize {
        let row = &v[y * cx as usize..(y + 1) * cx as usize];
        let mut x = 0;
        while x < row.len() {
            let p = row[x];
            let mut n = 1;
            while x + n < row.len() && row[x + n] == p && n < 255 {
                n += 1;
            }
            out.push(n as u8);
            out.push(if four { (p as u8 & 15) << 4 | (p as u8 & 15) } else { p as u8 });
            x += n;
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 1]);
    out
}

/// The colour bitmap of a form: palette indexes (screen and inverse pels
/// get the black entry) or RGB, rows from the bottom.
fn color_values(f: &Form) -> Vec<u32> {
    let black = f.palette.iter().position(|&c| c == 0).unwrap_or(0) as u32;
    let mut v = alloc::vec![0u32; (f.cx * f.cy) as usize];
    for y in 0..f.cy {
        for x in 0..f.cx {
            let p = f.pels[(y * f.cx + x) as usize];
            let o = ((f.cy - 1 - y) * f.cx + x) as usize;
            v[o] = if p & (SCREEN | INVERSE) != 0 {
                if f.bpp > 8 { 0 } else { black }
            } else if f.bpp > 8 {
                p & 0xffffff
            } else {
                f.nearest(p) as u32
            };
        }
    }
    v
}

/// The mask bitmap of an icon or pointer form (XOR then AND, rows from
/// the bottom). In a black and white form the colours become XOR bits.
fn mask_values(f: &Form, mono: bool) -> Vec<u32> {
    let (cx, cy) = (f.cx, f.cy);
    let mut v = alloc::vec![0u32; (cx * cy * 2) as usize];
    for y in 0..cy {
        for x in 0..cx {
            let p = f.pels[(y * cx + x) as usize];
            let (and, xor) = match p {
                SCREEN => (1, 0),
                INVERSE => (1, 1),
                c if mono => {
                    let l = (c >> 16 & 255) * 3 + (c >> 8 & 255) * 4 + (c & 255) * 2;
                    (0, (l >= 9 * 128) as u32)
                }
                _ => (0, 0),
            };
            let r = cy - 1 - y;
            v[(r * cx + x) as usize] = xor;
            v[((r + cy) * cx + x) as usize] = and;
        }
    }
    v
}

fn out_for(v: &[u32], cx: u32, cy: u32, bpp: u16, palette: Vec<u32>, packed: bool) -> Out {
    if packed && (bpp == 8 || bpp == 4) {
        Out { cx, cy, bpp, palette, bits: rle(v, cx, cy, bpp == 4), compression: if bpp == 4 { 2 } else { 1 } }
    } else {
        Out { cx, cy, bpp, palette, bits: pack(v, cx, cy, bpp), compression: 0 }
    }
}

/// The bitmaps of one image: one for a bitmap or a black and white icon,
/// the mask and the colours for a colour icon or pointer.
fn image_bitmaps(kind: Kind, f: &Form, packed: bool) -> (&'static [u8; 2], Vec<Out>) {
    match kind {
        Kind::Bitmap => (b"BM", alloc::vec![out_for(&color_values(f), f.cx, f.cy, f.bpp, f.palette.clone(), packed)]),
        Kind::Icon | Kind::Pointer => {
            let mono = f.bpp == 1;
            let ty: &'static [u8; 2] = match (kind, mono) {
                (Kind::Icon, true) => b"IC",
                (Kind::Icon, false) => b"CI",
                (_, true) => b"PT",
                _ => b"CP",
            };
            let mask = Out { cx: f.cx, cy: f.cy * 2, bpp: 1, palette: default_palette(1), bits: pack(&mask_values(f, mono), f.cx, f.cy * 2, 1), compression: 0 };
            let mut v = alloc::vec![mask];
            if !mono {
                v.push(out_for(&color_values(f), f.cx, f.cy, f.bpp, f.palette.clone(), packed));
            }
            (ty, v)
        }
    }
}

fn put16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes());
}

fn put32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}

/// Write a figure: the forms chosen (indexes) in a layout. The headers
/// (with their colour tables) come first, then all the bits.
pub fn write(fig: &Figure, which: &[usize], format: Format) -> Vec<u8> {
    let v2 = matches!(format, Format::V20 | Format::V20Packed);
    let packed = format == Format::V20Packed;
    let forms: Vec<&Form> = which.iter().filter_map(|&i| fig.forms.get(i)).collect();
    // A bitmap of one form is a plain file in every layout.
    let array = format != Format::V11 && !(fig.kind == Kind::Bitmap && forms.len() == 1);
    let forms = if format == Format::V11 { &forms[..forms.len().min(1)] } else { &forms[..] };
    let fix: usize = if v2 { 64 } else { 12 };
    let entry = if v2 { 4 } else { 3 };
    let file_header = 14 + fix;
    let images: Vec<(&[u8; 2], Vec<Out>, &Form)> = forms.iter().map(|f| {
        let (t, o) = image_bitmaps(fig.kind, f, packed);
        (t, o, *f)
    }).collect();
    // Header sizes, to place the bits.
    let header_len = |outs: &Vec<Out>| -> usize { outs.iter().map(|o| file_header + o.palette.len() * entry).sum::<usize>() + if array { 14 } else { 0 } };
    let headers_total: usize = images.iter().map(|(_, o, _)| header_len(o)).sum();
    let bits_total: usize = images.iter().flat_map(|(_, o, _)| o.iter()).map(|o| o.bits.len()).sum();
    let mut out = Vec::with_capacity(headers_total + bits_total);
    let mut bits_at = headers_total;
    let mut at = 0usize;
    for (n, (ty, outs, f)) in images.iter().enumerate() {
        let len = header_len(outs);
        if array {
            out.extend_from_slice(b"BA");
            put32(&mut out, (14 + file_header) as u32);
            put32(&mut out, if n + 1 < images.len() { (at + len) as u32 } else { 0 });
            put16(&mut out, f.display.0);
            put16(&mut out, f.display.1);
        }
        for o in outs.iter() {
            out.extend_from_slice(*ty);
            // A plain bitmap file's cbSize is the file's size, an icon's,
            // a pointer's and an array entry's the header's.
            put32(&mut out, if array || fig.kind != Kind::Bitmap { file_header as u32 } else { (headers_total + bits_total) as u32 });
            let (hx, hy) = f.hot;
            put16(&mut out, hx as i16 as u16);
            put16(&mut out, hy as i16 as u16);
            put32(&mut out, bits_at as u32);
            if v2 {
                put32(&mut out, 64);
                put32(&mut out, o.cx);
                put32(&mut out, o.cy);
                put16(&mut out, 1);
                put16(&mut out, o.bpp);
                put32(&mut out, o.compression);
                put32(&mut out, o.bits.len() as u32);
                put32(&mut out, 0);
                put32(&mut out, 0);
                put32(&mut out, o.palette.len() as u32);
                put32(&mut out, 0);
                out.extend_from_slice(&[0u8; 24]);
            } else {
                put32(&mut out, 12);
                put16(&mut out, o.cx as u16);
                put16(&mut out, o.cy as u16);
                put16(&mut out, 1);
                put16(&mut out, o.bpp);
            }
            for &c in &o.palette {
                out.extend_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8]);
                if v2 {
                    out.push(0);
                }
            }
            bits_at += o.bits.len();
        }
        at += len;
    }
    for (_, outs, _) in &images {
        for o in outs {
            out.extend_from_slice(&o.bits);
        }
    }
    out
}

/// A form as a pointer's bitmaps: the masks (1 bit, twice the height,
/// XOR then AND) and the colours (24 bits, black where the screen shows),
/// rows from the bottom, for WinCreatePointerIndirect.
pub fn pointer_bits(f: &Form) -> (Vec<u8>, Vec<u8>) {
    let mask = pack(&mask_values(f, false), f.cx, f.cy * 2, 1);
    let mut v = alloc::vec![0u32; (f.cx * f.cy) as usize];
    for y in 0..f.cy {
        for x in 0..f.cx {
            let p = f.pels[(y * f.cx + x) as usize];
            v[((f.cy - 1 - y) * f.cx + x) as usize] = if p & (SCREEN | INVERSE) != 0 { 0 } else { p & 0xffffff };
        }
    }
    (mask, pack(&v, f.cx, f.cy, 24))
}

/// A short description of a form: "32x32 16 colors".
pub fn describe(f: &Form) -> Vec<u8> {
    let colors = match f.bpp {
        1 => "2 colors",
        4 => "16 colors",
        8 => "256 colors",
        _ => "RGB",
    };
    let mut s = alloc::format!("{}x{} {}", f.cx, f.cy, colors);
    if f.display != (0, 0) {
        s.push_str(&alloc::format!(" ({}x{})", f.display.0, f.display.1));
    } else {
        s.push_str(" (Independent)");
    }
    s.into_bytes()
}

/// Self-checks: round trips through every layout and pel depth.
pub fn selftest(mut report: impl FnMut(&str, bool)) -> u32 {
    let mut fails = 0;
    let mut check = |name: &str, ok: bool| {
        if !ok {
            fails += 1;
        }
        report(name, ok);
    };
    let mut icon = Form::new(32, 32, 4, SCREEN);
    for i in 0..32 {
        icon.set(i, i, 0xff0000);
        icon.set(31 - i, i, 0x0000ff);
        icon.set(i, 5, INVERSE);
        icon.set(i, 20, 0x008000);
    }
    icon.hot = (3, 28);
    let mut half = Form::new(16, 16, 4, 0xffffff);
    half.set(1, 2, 0x000080);
    half.set(15, 15, SCREEN);
    let mut mono = Form::new(32, 32, 1, SCREEN);
    mono.set(0, 0, 0xffffff);
    mono.set(1, 0, 0);
    mono.set(2, 0, INVERSE);
    mono.display = (1024, 768);
    let mut big = Form::new(40, 40, 8, 0x000000);
    for y in 0..40 {
        for x in 0..40 {
            big.set(x, y, big.palette[((x * 7 + y * 3) % 256) as usize]);
        }
    }
    big.display = (1024, 768);
    let fig = Figure { kind: Kind::Icon, forms: alloc::vec![icon.clone(), half.clone(), mono.clone(), big.clone()], format: Format::V20 };
    for fmt in [Format::V12, Format::V20, Format::V20Packed] {
        let b = write(&fig, &[0, 1, 2, 3], fmt);
        match read(&b) {
            Ok(r) => {
                check("icon array: kind and count", r.kind == Kind::Icon && r.forms.len() == 4);
                check("icon array: format", r.format == fmt);
                for (a, b) in fig.forms.iter().zip(r.forms.iter()) {
                    check("icon array: form pels", a.pels == b.pels && a.cx == b.cx && a.cy == b.cy && a.bpp == b.bpp);
                    check("icon array: hot spot and display", a.hot == b.hot && a.display == b.display);
                }
            }
            Err(e) => check(e, false),
        }
    }
    let ptr = Figure { kind: Kind::Pointer, forms: alloc::vec![mono.clone()], format: Format::V11 };
    let b = write(&ptr, &[0], Format::V11);
    check("pointer 1.1: type PT", &b[0..2] == b"PT");
    match read(&b) {
        Ok(r) => check("pointer 1.1 round trip", r.kind == Kind::Pointer && r.forms[0].pels == mono.pels && r.forms[0].hot == mono.hot),
        Err(e) => check(e, false),
    }
    let mut rgb = Form::new(5, 3, 24, 0);
    for (i, p) in rgb.pels.iter_mut().enumerate() {
        *p = (i as u32 * 0x112233) & 0xffffff;
    }
    for bpp in [1u16, 4, 8] {
        let mut f = Form::new(13, 7, bpp, 0);
        let n = f.palette.len() as u32;
        for y in 0..7 {
            for x in 0..13 {
                f.set(x, y, f.palette[((x + y * 13) as u32 % n) as usize]);
            }
        }
        for fmt in [Format::V11, Format::V20, Format::V20Packed] {
            let bm = Figure { kind: Kind::Bitmap, forms: alloc::vec![f.clone()], format: fmt };
            match read(&write(&bm, &[0], fmt)) {
                Ok(r) => check("bitmap round trip", r.forms[0].pels == f.pels && r.kind == Kind::Bitmap),
                Err(e) => check(e, false),
            }
        }
    }
    let bm = Figure { kind: Kind::Bitmap, forms: alloc::vec![rgb.clone()], format: Format::V11 };
    let b = write(&bm, &[0], Format::V11);
    check("bitmap 1.1: cbSize is the file size", u32::from_le_bytes([b[2], b[3], b[4], b[5]]) as usize == b.len());
    match read(&b) {
        Ok(r) => check("24-bit bitmap round trip", r.forms[0].pels == rgb.pels),
        Err(e) => check(e, false),
    }
    check("garbage is refused", read(b"XXthis is not a bitmap at all.....").is_err());
    fails
}
