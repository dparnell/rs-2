//! Autodesk FLI and FLC animations.
//!
//! A file is a 128-byte header and a sequence of frame chunks
//! (type 0xF1FA), each holding sub-chunks that change the palette or the
//! pixels of the previous frame. After the last frame comes the ring
//! frame, which turns the last frame back into the first, so an animation
//! loops without decoding the first frame from scratch. FLC files may
//! have a prefix chunk (type 0xF100) before the first frame; it is
//! skipped, as are postage stamps (thumbnails).
//!
//! [`Decoder`] keeps the current frame as 8-bit palette indexes (`width`
//! × `height` bytes, rows top to bottom) and the 256-entry RGB palette.

use alloc::vec;
use alloc::vec::Vec;

use crate::{le16, le32};

pub const MAGIC_FLI: u16 = 0xAF11;
pub const MAGIC_FLC: u16 = 0xAF12;

const FRAME_TYPE: u16 = 0xF1FA;
const PREFIX_TYPE: u16 = 0xF100;

/// Sub-chunk types.
pub const COLOR_256: u16 = 4;
pub const DELTA_FLC: u16 = 7;
pub const COLOR_64: u16 = 11;
pub const DELTA_FLI: u16 = 12;
pub const BLACK: u16 = 13;
pub const BYTE_RUN: u16 = 15;
pub const FLI_COPY: u16 = 16;
pub const PSTAMP: u16 = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not an FLI or 8-bit FLC file.
    NotFlc,
    /// A chunk runs past the end of the file or of its frame.
    Truncated,
    /// A frame chunk with a type other than 0xF1FA.
    BadFrame,
}

/// The file header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// File size the header gives.
    pub size: u32,
    /// [`MAGIC_FLI`] or [`MAGIC_FLC`].
    pub magic: u16,
    /// Frames, not counting the ring frame.
    pub frames: u16,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub flags: u16,
    /// Delay between frames as stored: 1/70 s for FLI, ms for FLC.
    pub speed: u32,
    /// Offset of the first frame (FLC; 128 for FLI or when 0).
    pub oframe1: u32,
    /// Offset of the second frame (FLC; 0 when unknown).
    pub oframe2: u32,
}

impl Header {
    pub fn parse(d: &[u8]) -> Result<Header, Error> {
        let magic = le16(d, 4).ok_or(Error::NotFlc)?;
        if (magic != MAGIC_FLI && magic != MAGIC_FLC) || d.len() < 128 {
            return Err(Error::NotFlc);
        }
        let r16 = |at| le16(d, at).unwrap();
        let r32 = |at| le32(d, at).unwrap();
        let mut h = Header {
            size: r32(0),
            magic,
            frames: r16(6),
            width: r16(8),
            height: r16(10),
            depth: r16(12),
            flags: r16(14),
            speed: if magic == MAGIC_FLI { r16(16) as u32 } else { r32(16) },
            oframe1: 0,
            oframe2: 0,
        };
        if magic == MAGIC_FLC {
            h.oframe1 = r32(80);
            h.oframe2 = r32(84);
        }
        if h.depth != 8 && h.depth != 0 {
            return Err(Error::NotFlc);
        }
        if magic == MAGIC_FLI || h.width == 0 || h.height == 0 {
            // FLI is always 320x200.
            h.width = if h.width == 0 { 320 } else { h.width };
            h.height = if h.height == 0 { 200 } else { h.height };
        }
        Ok(h)
    }

    /// Delay between frames in milliseconds.
    pub fn delay_ms(&self) -> u32 {
        if self.magic == MAGIC_FLI { self.speed * 1000 / 70 } else { self.speed }
    }
}

/// What [`Decoder::next_frame`] decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The frame now in the buffer (0 = first).
    pub index: u16,
    /// The frame changed the palette.
    pub palette_changed: bool,
    /// The frame came from the ring frame (or a restart): the animation
    /// went round to its first frame.
    pub looped: bool,
    /// How long to show the frame, in milliseconds (a frame's own delay,
    /// else the header's speed).
    pub delay_ms: u32,
}

/// Decodes an animation frame by frame. `D` is the file contents
/// (a `Vec<u8>`, a slice...).
pub struct Decoder<D: AsRef<[u8]> = Vec<u8>> {
    data: D,
    header: Header,
    pixels: Vec<u8>,
    palette: [[u8; 3]; 256],
    /// Offset of the next frame chunk.
    pos: usize,
    /// Offset of the first frame chunk (after any prefix).
    first: usize,
    /// Offset of the second frame chunk, where decoding goes on after the
    /// ring frame.
    second: usize,
    /// Frames decoded since the start (the next frame's index when below
    /// `frames`).
    decoded: u32,
    /// Index of the frame in the buffer (None before the first).
    current: Option<u16>,
}

impl<D: AsRef<[u8]>> Decoder<D> {
    pub fn new(data: D) -> Result<Decoder<D>, Error> {
        let d = data.as_ref();
        let header = Header::parse(d)?;
        let mut first = if header.oframe1 >= 128 { header.oframe1 as usize } else { 128 };
        // Skip prefix (and any other non-frame) chunks before the first frame.
        while let (Some(size), Some(ty)) = (le32(d, first), le16(d, first + 4)) {
            if ty == FRAME_TYPE || size < 6 {
                break;
            }
            if ty != PREFIX_TYPE && header.oframe1 >= 128 {
                return Err(Error::BadFrame);
            }
            first += size as usize;
        }
        let pixels = vec![0; header.width as usize * header.height as usize];
        Ok(Decoder { data, header, pixels, palette: [[0; 3]; 256], pos: first, first, second: 0, decoded: 0, current: None })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn width(&self) -> usize {
        self.header.width as usize
    }

    pub fn height(&self) -> usize {
        self.header.height as usize
    }

    /// Frames in the animation (not counting the ring frame).
    pub fn frame_count(&self) -> u16 {
        self.header.frames
    }

    /// The frame in the buffer: `width * height` palette indexes, rows top
    /// to bottom.
    pub fn frame(&self) -> &[u8] {
        &self.pixels
    }

    /// The palette (RGB, 0-255 each).
    pub fn palette(&self) -> &[[u8; 3]; 256] {
        &self.palette
    }

    /// The frame index in the buffer, None before the first `next_frame`.
    pub fn current(&self) -> Option<u16> {
        self.current
    }

    /// Go back to before the first frame (black, palette cleared).
    pub fn rewind(&mut self) {
        self.pos = self.first;
        self.decoded = 0;
        self.current = None;
        self.pixels.fill(0);
        self.palette = [[0; 3]; 256];
    }

    /// Decode the next frame. After the last frame the ring frame is
    /// decoded, giving frame 0 again (`looped`); a file without a ring
    /// frame starts again from the first frame.
    pub fn next_frame(&mut self) -> Result<Frame, Error> {
        let frames = self.header.frames.max(1) as u32;
        let mut looped = false;
        if self.decoded > frames || le32(self.data.as_ref(), self.pos).is_none() {
            // Past the ring frame, or no ring frame: restart.
            if self.decoded == 0 {
                return Err(Error::Truncated);
            }
            self.rewind();
            looped = true;
        }
        let (palette_changed, delay) = self.decode_at(self.pos)?;
        let size = le32(self.data.as_ref(), self.pos).unwrap() as usize;
        self.pos += size.max(16);
        self.decoded += 1;
        if self.decoded == 1 {
            self.second = self.pos;
        }
        let index = if self.decoded > frames {
            // The ring frame: it gives frame 0; frame 1 follows it.
            looped = true;
            self.decoded = 1;
            self.pos = self.second;
            0
        } else {
            (self.decoded - 1) as u16
        };
        self.current = Some(index);
        let delay_ms = if delay != 0 { delay as u32 } else { self.header.delay_ms() };
        Ok(Frame { index, palette_changed, looped, delay_ms })
    }

    /// Decode the frame chunk at `at`: (palette changed, frame delay).
    fn decode_at(&mut self, at: usize) -> Result<(bool, u16), Error> {
        let d = self.data.as_ref();
        let size = le32(d, at).ok_or(Error::Truncated)? as usize;
        let ty = le16(d, at + 4).ok_or(Error::Truncated)?;
        if ty != FRAME_TYPE {
            return Err(Error::BadFrame);
        }
        let end = at.checked_add(size).filter(|&e| e <= d.len() && size >= 16).ok_or(Error::Truncated)?;
        let chunks = le16(d, at + 6).unwrap();
        let delay = le16(d, at + 8).unwrap();
        let frame = &d[..end];
        let mut p = at + 16;
        let mut palette_changed = false;
        let w = self.header.width as usize;
        let h = self.header.height as usize;
        let fli = self.header.magic == MAGIC_FLI;
        for _ in 0..chunks {
            let csize = le32(frame, p).ok_or(Error::Truncated)? as usize;
            let ctype = le16(frame, p + 4).ok_or(Error::Truncated)?;
            if csize < 6 {
                return Err(Error::Truncated);
            }
            // FLI files sometimes give a chunk size a little past the frame.
            let cend = p.checked_add(csize).ok_or(Error::Truncated)?.min(end);
            let body = &frame[p + 6..cend];
            let px = &mut self.pixels;
            match ctype {
                COLOR_256 => {
                    color(body, &mut self.palette, false)?;
                    palette_changed = true;
                }
                COLOR_64 => {
                    color(body, &mut self.palette, true)?;
                    palette_changed = true;
                }
                DELTA_FLC => delta_flc(body, px, w, h)?,
                DELTA_FLI => delta_fli(body, px, w, h)?,
                BLACK => px.fill(0),
                BYTE_RUN => byte_run(body, px, w, h)?,
                FLI_COPY => {
                    let n = px.len().min(body.len());
                    px[..n].copy_from_slice(&body[..n]);
                }
                _ => {} // PSTAMP and others (FLC's DTA chunks): skip.
            }
            p += if fli { csize } else { csize + (csize & 1) }.max(6);
            if p >= end {
                break;
            }
        }
        Ok((palette_changed, delay))
    }
}

/// A cursor over a chunk's data.
struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> Result<u8, Error> {
        let b = *self.d.get(self.p).ok_or(Error::Truncated)?;
        self.p += 1;
        Ok(b)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let v = le16(self.d, self.p).ok_or(Error::Truncated)?;
        self.p += 2;
        Ok(v)
    }

    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let s = self.d.get(self.p..self.p + n).ok_or(Error::Truncated)?;
        self.p += n;
        Ok(s)
    }
}

/// COLOR_256 / COLOR_64: packets of (colours to skip, colours to set
/// (0 = 256), RGB triples).
fn color(d: &[u8], pal: &mut [[u8; 3]; 256], six_bit: bool) -> Result<(), Error> {
    let mut r = Rd { d, p: 0 };
    let packets = r.u16()?;
    let mut i = 0usize;
    for _ in 0..packets {
        i += r.u8()? as usize;
        let n = match r.u8()? {
            0 => 256,
            n => n as usize,
        };
        for _ in 0..n {
            let rgb = r.take(3)?;
            if let Some(e) = pal.get_mut(i) {
                for (c, &v) in e.iter_mut().zip(rgb) {
                    *c = if six_bit { (v << 2) | (v >> 4) } else { v };
                }
            }
            i += 1;
        }
    }
    Ok(())
}

/// Write `src` at `x` in a row, clipped to the row.
fn put(row: &mut [u8], x: usize, src: &[u8]) {
    if x < row.len() {
        let n = src.len().min(row.len() - x);
        row[x..x + n].copy_from_slice(&src[..n]);
    }
}

fn fill(row: &mut [u8], x: usize, n: usize, v: u8) {
    if x < row.len() {
        let n = n.min(row.len() - x);
        row[x..x + n].fill(v);
    }
}

/// DELTA_FLI (LC): lines to skip, lines changed; per line packets of
/// (columns to skip, count: > 0 copy count bytes, < 0 repeat one byte).
fn delta_fli(d: &[u8], px: &mut [u8], w: usize, h: usize) -> Result<(), Error> {
    let mut r = Rd { d, p: 0 };
    let mut y = r.u16()? as usize;
    let lines = r.u16()?;
    for _ in 0..lines {
        let packets = r.u8()?;
        let row = if y < h { &mut px[y * w..(y + 1) * w] } else { &mut [][..] };
        let mut x = 0usize;
        for _ in 0..packets {
            x += r.u8()? as usize;
            let n = r.u8()? as i8;
            if n >= 0 {
                put(row, x, r.take(n as usize)?);
                x += n as usize;
            } else {
                let v = r.u8()?;
                fill(row, x, n.unsigned_abs() as usize, v);
                x += n.unsigned_abs() as usize;
            }
        }
        y += 1;
    }
    Ok(())
}

/// DELTA_FLC (SS2): lines with word packets. Each line starts with words
/// whose top bits say: 11 skip -n lines, 10 set the line's last pixel to
/// the low byte, 00 the packet count. Packets: (columns to skip,
/// count: > 0 copy count words, < 0 repeat one word).
fn delta_flc(d: &[u8], px: &mut [u8], w: usize, h: usize) -> Result<(), Error> {
    let mut r = Rd { d, p: 0 };
    let lines = r.u16()?;
    let mut y = 0usize;
    for _ in 0..lines {
        let packets = loop {
            let op = r.u16()?;
            match op >> 14 {
                0 => break op,
                3 => y += (op as i16).unsigned_abs() as usize,
                2 => {
                    if y < h && w > 0 {
                        px[y * w + w - 1] = op as u8;
                    }
                }
                _ => return Err(Error::Truncated),
            }
        };
        let row = if y < h { &mut px[y * w..(y + 1) * w] } else { &mut [][..] };
        let mut x = 0usize;
        for _ in 0..packets {
            x += r.u8()? as usize;
            let n = r.u8()? as i8;
            if n >= 0 {
                put(row, x, r.take(2 * n as usize)?);
                x += 2 * n as usize;
            } else {
                let v = r.take(2)?;
                for _ in 0..n.unsigned_abs() {
                    put(row, x, v);
                    x += 2;
                }
            }
        }
        y += 1;
    }
    Ok(())
}

/// BYTE_RUN (BRUN): every line, a (ignored) packet count, then runs until
/// the line is full: count > 0 repeat the next byte, < 0 copy -count bytes.
fn byte_run(d: &[u8], px: &mut [u8], w: usize, h: usize) -> Result<(), Error> {
    let mut r = Rd { d, p: 0 };
    for y in 0..h {
        r.u8()?;
        let row = &mut px[y * w..(y + 1) * w];
        let mut x = 0usize;
        while x < w {
            let n = r.u8()? as i8;
            if n >= 0 {
                let v = r.u8()?;
                fill(row, x, n as usize, v);
                x += n as usize;
            } else {
                let n = n.unsigned_abs() as usize;
                put(row, x, r.take(n)?);
                x += n;
            }
            if n == 0 {
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny FLC: 4x2, 2 frames + ring frame.
    fn sample() -> Vec<u8> {
        let mut f = vec![0u8; 128];
        f[4..6].copy_from_slice(&MAGIC_FLC.to_le_bytes());
        f[6..8].copy_from_slice(&2u16.to_le_bytes());
        f[8..10].copy_from_slice(&4u16.to_le_bytes());
        f[10..12].copy_from_slice(&2u16.to_le_bytes());
        f[12..14].copy_from_slice(&8u16.to_le_bytes());
        f[16..20].copy_from_slice(&50u32.to_le_bytes());
        f[80..84].copy_from_slice(&128u32.to_le_bytes());
        let chunk = |ty: u16, body: &[u8]| {
            let mut c = Vec::new();
            let size = 6 + body.len() + (body.len() & 1);
            c.extend_from_slice(&(size as u32).to_le_bytes());
            c.extend_from_slice(&ty.to_le_bytes());
            c.extend_from_slice(body);
            if body.len() & 1 != 0 {
                c.push(0);
            }
            c
        };
        let frame = |chunks: &[Vec<u8>]| {
            let body: Vec<u8> = chunks.concat();
            let mut c = Vec::new();
            c.extend_from_slice(&(16 + body.len() as u32).to_le_bytes());
            c.extend_from_slice(&FRAME_TYPE.to_le_bytes());
            c.extend_from_slice(&(chunks.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0; 8]);
            c.extend_from_slice(&body);
            c
        };
        // Frame 0: palette entry 1 = red, 2 = 6-bit white; BRUN rows.
        let pal = chunk(COLOR_256, &[1, 0, 1, 1, 255, 0, 0]);
        let pal64 = chunk(COLOR_64, &[1, 0, 2, 1, 63, 63, 63]);
        // row 0: 4 x 1; row 1: copy 1,2,1,2
        let brun = chunk(BYTE_RUN, &[1, 4, 1, 1, 0xfc, 1, 2, 1, 2]);
        // Frame 1: SS2: 2 lines; line 0: 1 packet skip 1 copy 1 word (2,2);
        // line 1: set last pixel to 0 then 1 packet skip 0 repeat word (0,0) once.
        let ss2 = chunk(DELTA_FLC, &[2, 0, 1, 0, 1, 1, 2, 2, 0x00, 0x80, 1, 0, 0, 0xff, 0, 0]);
        // Ring frame: LC: skip 0 lines, 2 lines; line 0: 1 packet skip 0, repeat 1 x4;
        // line 1: 1 packet skip 0 copy 4 bytes.
        let lc = chunk(DELTA_FLI, &[0, 0, 2, 0, 1, 0, 0xfc, 1, 1, 0, 4, 1, 2, 1, 2]);
        f.extend(frame(&[pal, pal64, brun]));
        f.extend(frame(&[ss2]));
        f.extend(frame(&[lc]));
        let len = f.len() as u32;
        f[0..4].copy_from_slice(&len.to_le_bytes());
        f
    }

    #[test]
    fn decodes_and_loops() {
        let mut d = Decoder::new(sample()).unwrap();
        assert_eq!((d.width(), d.height(), d.frame_count()), (4, 2, 2));
        let f = d.next_frame().unwrap();
        assert_eq!((f.index, f.palette_changed, f.looped, f.delay_ms), (0, true, false, 50));
        assert_eq!(d.frame(), &[1, 1, 1, 1, 1, 2, 1, 2]);
        assert_eq!(d.palette()[1], [255, 0, 0]);
        assert_eq!(d.palette()[2], [255, 255, 255]);
        let first = d.frame().to_vec();
        let f = d.next_frame().unwrap();
        assert_eq!((f.index, f.looped), (1, false));
        assert_eq!(d.frame(), &[1, 2, 2, 1, 0, 0, 1, 0]);
        let f = d.next_frame().unwrap();
        assert_eq!((f.index, f.looped), (0, true));
        assert_eq!(d.frame(), &first[..]);
        let f = d.next_frame().unwrap();
        assert_eq!(f.index, 1);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(Decoder::new(vec![0u8; 200]).err(), Some(Error::NotFlc));
        let mut s = sample();
        s.truncate(150);
        let mut d = Decoder::new(s).unwrap();
        assert_eq!(d.next_frame(), Err(Error::Truncated));
    }
}
