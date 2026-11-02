//! IBM Ultimotion ("ULTI"), the software video codec of OS/2's MMPM/2
//! (decoded on OS/2 by ULDC.DLL).
//!
//! The picture is YUV with 6-bit luma per pixel and one chroma byte (4-bit
//! Cb in the high nibble, 4-bit Cr in the low) per 4×4 pixels. A frame
//! is a byte stream walking 8×8 macroblocks left to right, top to
//! bottom; each holds four 4×4 sub-blocks in the order top-left,
//! bottom-left, bottom-right, top-right. Bytes 0x70..0x77 are escapes:
//!
//! - 0x70 m: set the modifier (0 = codebook mode, 1 = direct luma);
//! - 0x71: the next macroblock has one chroma byte per coded sub-block;
//! - 0x72: toggle that for all following macroblocks;
//! - 0x73: end of frame;
//! - 0x74 n: skip n macroblocks (they keep the previous frame).
//!
//! Any other byte starts a macroblock: four 2-bit codes, first sub-block
//! in the top bits, then (unless per-sub-block chroma is on) a chroma
//! byte when any code is nonzero. Per sub-block, after its own chroma
//! byte when that is on:
//!
//! - code 0: unchanged;
//! - code 1: one byte, 2 bits of angle (none, 2, 6 or reversed 4) and a
//!   6-bit luma Y; the block is flat Y, or a gradient from Y to Y + 1;
//! - code 2: modifier 0: 16 bits, a 4-bit angle and a 12-bit codebook
//!   index giving four lumas, drawn as a gradient; modifier 1: 24 bits,
//!   four 6-bit lumas for the four 2×2 quarters;
//! - code 3: modifier 1: 4 × 24 bits, the 16 lumas in raster order;
//!   modifier 0: a byte with the top bit set starts a gradient (3-bit
//!   angle, four 6-bit lumas in 4 bytes), clear starts a two-level
//!   pattern (16 mask bits, set = second luma, then two lumas).
//!
//! A gradient draws four lumas Y0..Y3 across the block at one of eight
//! angles (in steps of 22.5°; angle bit 3 reverses the lumas). The
//! codebook (4096 entries of four non-decreasing lumas) is IBM's data,
//! not reproduced here: [`find_codebook`] extracts it from the decoded
//! code object of IBM's ULDC.DLL and [`Ulti::set_codebook`] installs it.
//! Without it, codebook blocks are drawn flat at a level estimated from
//! the index (entries are ordered by their first luma), which gives a
//! recognizable but blocky picture.

use alloc::vec;
use alloc::vec::Vec;

use mmimage::Image;

use crate::Error;

/// The gradients: for each angle, the luma (0..3) of each pixel in
/// raster order.
const GRADIENTS: [[u8; 16]; 8] = [
    [0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3],
    [1, 2, 3, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 0, 1, 2],
    [1, 2, 3, 3, 1, 2, 2, 3, 0, 1, 1, 2, 0, 0, 1, 2],
    [2, 3, 3, 3, 1, 2, 2, 3, 0, 1, 1, 2, 0, 0, 0, 1],
    [3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0],
    [3, 3, 3, 2, 3, 2, 2, 1, 2, 1, 1, 0, 1, 0, 0, 0],
    [3, 3, 2, 2, 3, 2, 1, 1, 2, 2, 1, 0, 1, 1, 0, 0],
    [3, 3, 2, 1, 3, 2, 1, 0, 3, 2, 1, 0, 2, 1, 0, 0],
];

/// Four 2×2 quarters (modifier 1, code 2).
const QUARTERS: [u8; 16] = [0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 3, 3, 2, 2, 3, 3];

/// Sub-block positions within a macroblock.
const SUB: [(usize, usize); 4] = [(0, 0), (0, 4), (4, 4), (4, 0)];

/// The 16 chroma levels.
const CHROMA: [u8; 16] = [96, 103, 109, 115, 122, 128, 134, 141, 147, 153, 160, 166, 172, 179, 185, 192];

/// 6-bit luma to 8-bit video range (16..235).
fn luma(v: u8) -> u8 { (16 + ((v as u32 & 63) * 219 + 31) / 63) as u8 }

pub struct Ulti {
    width: usize,
    height: usize,
    /// Padded to whole macroblocks.
    pw: usize,
    ph: usize,
    /// 8-bit luma per pixel; Cb and Cr per 4×4 pixels.
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
    codebook: Option<Vec<u8>>,
}

impl Ulti {
    pub fn new(width: u32, height: u32) -> Ulti {
        let (pw, ph) = ((width as usize).div_ceil(8) * 8, (height as usize).div_ceil(8) * 8);
        Ulti { width: width as usize, height: height as usize, pw, ph, y: vec![16; pw * ph], cb: vec![128; pw * ph / 16], cr: vec![128; pw * ph / 16], codebook: None }
    }

    /// Installs the codebook: 4096 × 4 lumas (0..63).
    pub fn set_codebook(&mut self, cb: &[u8]) -> bool {
        if cb.len() != 16384 { return false; }
        self.codebook = Some(cb.iter().map(|&v| v & 63).collect());
        true
    }

    pub fn has_codebook(&self) -> bool { self.codebook.is_some() }

    /// The planes: luma (padded width × height), Cb and Cr (a quarter of
    /// that each way).
    pub fn planes(&self) -> (&[u8], &[u8], &[u8], usize) { (&self.y, &self.cb, &self.cr, self.pw) }

    fn put(&mut self, x: usize, y: usize, l: &[u8; 16], chroma: u8) {
        for r in 0..4 {
            let at = (y + r) * self.pw + x;
            for c in 0..4 { self.y[at + c] = luma(l[r * 4 + c]); }
        }
        let at = (y / 4) * (self.pw / 4) + x / 4;
        self.cb[at] = CHROMA[(chroma >> 4) as usize];
        self.cr[at] = CHROMA[(chroma & 15) as usize];
    }

    fn gradient(&mut self, x: usize, y: usize, mut v: [u8; 4], angle: u8, chroma: u8) {
        if angle & 8 != 0 { v.reverse(); }
        let pattern = if angle >= 16 { &QUARTERS } else { &GRADIENTS[(angle & 7) as usize] };
        let l = pattern.map(|i| v[i as usize]);
        self.put(x, y, &l, chroma);
    }

    /// Decodes a frame over the previous one.
    pub fn decode(&mut self, d: &[u8]) -> Result<(), Error> {
        let (bw, bh) = (self.pw / 8, self.ph / 8);
        let total = bw * bh;
        let mut i = 0;
        let next = |i: &mut usize| -> Result<u8, Error> { let b = *d.get(*i).ok_or(Error::Truncated)?; *i += 1; Ok(b) };
        let (mut modifier, mut per_sub, mut once) = (0u8, false, false);
        let mut block = 0;
        while block < total {
            let Some(&b) = d.get(i) else { break };
            i += 1;
            if b & 0xF8 == 0x70 {
                match b {
                    0x70 => modifier = next(&mut i)?,
                    0x71 => once = true,
                    0x72 => per_sub = !per_sub,
                    0x73 => break,
                    0x74 => block += next(&mut i)? as usize,
                    _ => {}
                }
                continue;
            }
            let (mx, my) = ((block % bw) * 8, (block / bw) * 8);
            let each = per_sub || once;
            once = false;
            let mut chroma = if !each && b != 0 { next(&mut i)? } else { 0 };
            for (s, &(sx, sy)) in SUB.iter().enumerate() {
                let code = (b >> (6 - 2 * s)) & 3;
                if code == 0 { continue; }
                if each { chroma = next(&mut i)?; }
                let (x, y) = (mx + sx, my + sy);
                match code {
                    1 => {
                        let t = next(&mut i)?;
                        let angle = [0, 2, 6, 12][(t >> 6) as usize];
                        let v = t & 63;
                        let w = if angle != 0 { (v + 1).min(63) } else { v };
                        self.gradient(x, y, [v, v, w, w], angle, chroma);
                    }
                    2 if modifier != 0 => {
                        let t = u32::from_be_bytes([0, next(&mut i)?, next(&mut i)?, next(&mut i)?]);
                        self.gradient(x, y, [(t >> 18) as u8 & 63, (t >> 12) as u8 & 63, (t >> 6) as u8 & 63, t as u8 & 63], 16, chroma);
                    }
                    2 => {
                        let t = u16::from_be_bytes([next(&mut i)?, next(&mut i)?]);
                        let k = (t & 0xFFF) as usize;
                        let v = match &self.codebook {
                            Some(cb) => [cb[k * 4], cb[k * 4 + 1], cb[k * 4 + 2], cb[k * 4 + 3]],
                            None => [(k >> 6) as u8; 4],
                        };
                        self.gradient(x, y, v, (t >> 12) as u8, chroma);
                    }
                    _ if modifier != 0 => {
                        let mut l = [0u8; 16];
                        for q in 0..4 {
                            let t = u32::from_be_bytes([0, next(&mut i)?, next(&mut i)?, next(&mut i)?]);
                            l[q * 4..q * 4 + 4].copy_from_slice(&[(t >> 18) as u8 & 63, (t >> 12) as u8 & 63, (t >> 6) as u8 & 63, t as u8 & 63]);
                        }
                        self.put(x, y, &l, chroma);
                    }
                    _ => {
                        let t = next(&mut i)?;
                        if t & 0x80 != 0 {
                            let u = next(&mut i)?;
                            let v = [((t & 15) << 2) | (u >> 6), u & 63, next(&mut i)? & 63, next(&mut i)? & 63];
                            self.gradient(x, y, v, (t >> 4) & 7, chroma);
                        } else {
                            let mask = (t as u16) << 8 | next(&mut i)? as u16;
                            let (a, c) = (next(&mut i)? & 63, next(&mut i)? & 63);
                            let l: [u8; 16] = core::array::from_fn(|p| if mask & (0x8000 >> p) != 0 { c } else { a });
                            self.put(x, y, &l, chroma);
                        }
                    }
                }
            }
            block += 1;
        }
        Ok(())
    }

    /// Converts the picture to RGB (ITU-R BT.601, video range) into a
    /// 24-bit image of the frame's size.
    pub fn to_rgb(&self, img: &mut Image) {
        let cw = self.pw / 4;
        for y in 0..self.height.min(img.height as usize) {
            let row = img.row_mut(y as u32);
            for x in 0..self.width {
                let c = 298 * (self.y[y * self.pw + x] as i32 - 16);
                let at = (y / 4) * cw + x / 4;
                let (d, e) = (self.cb[at] as i32 - 128, self.cr[at] as i32 - 128);
                let r = (c + 409 * e + 128) >> 8;
                let g = (c - 100 * d - 208 * e + 128) >> 8;
                let b = (c + 516 * d + 128) >> 8;
                if let Some(p) = row.get_mut(x * 3..x * 3 + 3) {
                    p[0] = b.clamp(0, 255) as u8;
                    p[1] = g.clamp(0, 255) as u8;
                    p[2] = r.clamp(0, 255) as u8;
                }
            }
        }
    }
}

/// Finds the Ultimotion codebook in a decoded code/data object of IBM's
/// ULDC.DLL: 16384 bytes holding 4096 groups of four non-decreasing
/// lumas, stored × 4 (the low two bits are flags), the groups ordered by
/// their first luma from 0. Returns the lumas (0..63).
pub fn find_codebook(obj: &[u8]) -> Option<Vec<u8>> {
    const N: usize = 16384;
    if obj.len() < N { return None; }
    let ok = |s: usize| -> bool {
        let t = &obj[s..s + N];
        if t[0] >> 2 != 0 || t[N - 4] >> 2 < 48 { return false; }
        let mut prev = 0;
        for g in t.chunks_exact(4) {
            let v = [g[0] >> 2, g[1] >> 2, g[2] >> 2, g[3] >> 2];
            if v[0] < prev || v[0] > v[1] || v[1] > v[2] || v[2] > v[3] { return false; }
            prev = v[0];
        }
        // Not a constant or trivially repetitive run.
        t.chunks_exact(4).filter(|g| g[3] >> 2 > g[0] >> 2).count() > 3000
    };
    (0..=obj.len() - N).step_by(4).find(|&s| ok(s)).map(|s| obj[s..s + N].iter().map(|b| b >> 2).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;

    /// A one-frame ULTI AVI of `w`×`h` (via the test AVI builder).
    fn avi(w: i32, h: i32, frames: &[Vec<u8>]) -> Vec<u8> { build(w, h, 16, b"ULTI", &[], frames, &[], true) }

    #[test]
    fn blocks() {
        let mut u = Ulti::new(16, 8);
        // Macroblock 0: code 3 (modifier 1) in the first sub-block with
        // lumas 0..15; macroblock 1: code 1 in all four, flat.
        let mut f = vec![0x70, 1, 0xC0, 0x12];
        for q in 0..4u32 {
            let v: Vec<u32> = (0..4).map(|k| q * 4 + k).collect();
            f.extend_from_slice(&((v[0] << 18) | (v[1] << 12) | (v[2] << 6) | v[3]).to_be_bytes()[1..]);
        }
        f.extend_from_slice(&[0x55, 0x00, 10, 20, 30, 40, 0x73]);
        u.decode(&f).unwrap();
        let (y, cb, cr, pw) = u.planes();
        assert_eq!(y[0], luma(0));
        assert_eq!(y[3 * pw + 3], luma(15));
        assert_eq!((cb[0], cr[0]), (CHROMA[1], CHROMA[2]));
        // Sub-block order: top-left, bottom-left, bottom-right, top-right.
        assert_eq!(y[8], luma(10));
        assert_eq!(y[4 * pw + 8], luma(20));
        assert_eq!(y[4 * pw + 12], luma(30));
        assert_eq!(y[12], luma(40));
        assert!(u.decode(&[0x55, 0x00, 1]).is_err());
        assert_eq!(luma(63), 235);
    }

    /// Decodes `avi` with ffmpeg (when installed) to YUV 4:1:0 planes.
    fn ffmpeg_yuv(name: &str, data: &[u8]) -> Option<Vec<u8>> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mmavi-test");
        std::fs::create_dir_all(&dir).ok()?;
        let p = dir.join(name);
        std::fs::write(&p, data).ok()?;
        let o = std::process::Command::new("ffmpeg").args(["-loglevel", "error", "-i"]).arg(&p)
            .args(["-f", "rawvideo", "-pix_fmt", "yuv410p", "-"]).output().ok()?;
        o.status.success().then_some(o.stdout)
    }

    /// The reference codebook as ffmpeg decodes it, recovered by
    /// decoding a frame that draws every entry at angle 0 (test only).
    fn ffmpeg_codebook() -> Option<Vec<u8>> {
        let (w, h) = (256usize, 256);
        let mut f = vec![0x70, 0];
        for k in (0..4096u16).step_by(4) {
            f.extend_from_slice(&[0xAA, 0x55]);
            for j in 0..4 { f.extend_from_slice(&(k + j).to_be_bytes()); }
        }
        f.push(0x73);
        let yuv = ffmpeg_yuv("codebook.avi", &avi(w as i32, h, &[f]))?;
        let inv = |v: u8| (0..64u8).find(|&i| luma(i) == v);
        let mut cb = vec![0u8; 16384];
        for k in 0..4096 {
            let (blk, s) = (k / 4, k % 4);
            let (x, y) = ((blk % 32) * 8 + SUB[s].0, (blk / 32) * 8 + SUB[s].1);
            for j in 0..4 { cb[k * 4 + j] = inv(yuv[y * w + x + j])?; }
        }
        Some(cb)
    }

    fn uldc_codebook() -> Option<Vec<u8>> {
        let dll = work("unpacked/disk_17/MMOS2/DLL/ULDC.DLL")?;
        let Ok(os2_exe::Format::Lx { header }) = os2_exe::detect(&dll) else { return None };
        let m = os2_exe::lx::Module::parse(&dll, header).ok()?;
        let ps = m.header.page_size as usize;
        for o in m.objects().ok()? {
            let mut img = vec![0u8; o.page_count as usize * ps];
            for i in 0..o.page_count {
                m.load_page(o.page_index + i, &mut img[i as usize * ps..(i as usize + 1) * ps]).ok()?;
            }
            if let Some(cb) = find_codebook(&img) { return Some(cb); }
        }
        None
    }

    #[test]
    fn uldc_codebook_found() {
        if work("unpacked/disk_17/MMOS2/DLL/ULDC.DLL").is_none() { return; }
        let cb = uldc_codebook().expect("codebook in ULDC.DLL");
        assert_eq!(&cb[..8], &[0, 1, 1, 2, 0, 1, 2, 3]);
    }

    #[test]
    fn synthetic_against_ffmpeg() {
        // Every block type, angles and chroma modes against ffmpeg.
        let mut f = vec![0x70, 0];
        for a in 0..8u16 { f.extend_from_slice(&[0xC0, 0x00]); f.extend_from_slice(&(0x8000 | a << 12 | 20).to_be_bytes()); f.extend_from_slice(&[40, 60]); }
        for a in 0..16u16 { f.extend_from_slice(&[0x80, 0x13]); f.extend_from_slice(&(a << 12 | 1).to_be_bytes()); }
        f.extend_from_slice(&[0xC0, 0x21, 0x5A, 0x3C, 7, 50]);
        f.extend_from_slice(&[0x70, 1, 0x80, 0x45]);
        f.extend_from_slice(&((10u32 << 18) | (20 << 12) | (30 << 6) | 40).to_be_bytes()[1..]);
        for i in 0..4u8 { f.extend_from_slice(&[0x40, 0x00, (i << 6) | 63]); }
        f.extend_from_slice(&[0x71, 0x55, 0x12, 10, 0x34, 20, 0x56, 30, 0x78, 40]);
        f.extend_from_slice(&[0x72, 0x55, 0x9a, 10, 0xbc, 20, 0xde, 30, 0xf0, 40, 0x72, 0x74, 3, 0x55, 0xAB, 50, 51, 52, 53, 0x73]);
        let data = avi(320, 64, &[f.clone()]);
        let Some(yuv) = ffmpeg_yuv("synthetic.avi", &data) else { return };
        let mut u = Ulti::new(320, 64);
        if let Some(cb) = ffmpeg_codebook() { u.set_codebook(&cb); }
        u.decode(&f).unwrap();
        let (y, cb, cr, _) = u.planes();
        let n = 320 * 64;
        // Untouched blocks: ffmpeg starts from 0, we from black (16).
        for k in 0..n { if yuv[k] != 0 { assert_eq!(y[k], yuv[k], "luma at {},{}", k % 320, k / 320); } }
        // Untouched blocks: ffmpeg starts from 0, we from black.
        for k in 0..n / 16 {
            if yuv[n + k] != 0 { assert_eq!((cb[k], cr[k]), (yuv[n + k], yuv[n + n / 16 + k]), "chroma {k}"); }
        }
    }

    #[test]
    fn ibm_videos() {
        // IBM's sample videos against ffmpeg, frame by frame, with the
        // codebook ffmpeg uses; then report how IBM's own codebook (from
        // ULDC.DLL) compares.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mmavi-test");
        let _ = std::fs::create_dir_all(&root);
        let files: Vec<(&str, Vec<u8>)> = [
            ("macaw", macaw()),
            ("movie", work("media/boot/toolkit/os2tk45/samples/mm/movie/movie.avi")),
            ("ultieyes", work("media/boot/toolkit/os2tk45/samples/mm/ultieyes/ultieyes.avi")),
        ].into_iter().filter_map(|(n, d)| Some((n, d?))).collect();
        let ffcb = ffmpeg_codebook();
        let ibm = uldc_codebook();
        if let (Some(a), Some(b)) = (&ffcb, &ibm) {
            let same = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x == y).count();
            eprintln!("ULDC.DLL codebook: {same}/4096 entries equal to ffmpeg's");
        }
        for (name, data) in files {
            let a = crate::Avi::parse(&data).unwrap();
            assert_eq!(&a.video_fourcc(), b"ULTI");
            let (w, h) = (a.width() as usize, a.height() as usize);
            let mut dec = crate::VideoDecoder::new(&a).unwrap();
            let mut ibm_dec = crate::VideoDecoder::new(&a).unwrap();
            if let Some(cb) = &ffcb { dec.ulti().unwrap().set_codebook(cb); }
            if let Some(cb) = &ibm { ibm_dec.ulti().unwrap().set_codebook(cb); }
            let yuv = ffmpeg_yuv(&format!("{name}.avi"), &data);
            let fsize = w * h + 2 * (w / 4) * (h / 4);
            let (mut diff_ibm, mut px, mut max_ibm) = (0u64, 0u64, 0u32);
            let mut tv = (0u64, 0u64);
            for n in 0..a.frame_count() {
                let (o, l) = a.frame_chunk(n).unwrap();
                let chunk = &data[o as usize..o as usize + l as usize];
                dec.decode(chunk).unwrap();
                ibm_dec.decode(chunk).unwrap();
                let u = dec.ulti().unwrap();
                let (y, cb, cr, pw) = u.planes();
                let (y, cb, cr) = (y.to_vec(), cb.to_vec(), cr.to_vec());
                let (yi, ..) = ibm_dec.ulti().unwrap().planes();
                for r in 0..h {
                    for c in 0..w {
                        let d = (y[r * pw + c] as i32 - yi[r * pw + c] as i32).unsigned_abs();
                        diff_ibm += d as u64;
                        max_ibm = max_ibm.max(d);
                        px += 1;
                        // Total variation: the codebook that fits the
                        // encoder's gives the smoother picture.
                        if c + 1 < w && r + 1 < h {
                            let t = |p: &[u8]| (p[r * pw + c] as i32 - p[r * pw + c + 1] as i32).unsigned_abs() as u64 + (p[r * pw + c] as i32 - p[(r + 1) * pw + c] as i32).unsigned_abs() as u64;
                            tv.0 += t(&y);
                            tv.1 += t(yi);
                        }
                    }
                }
                if let (Some(yuv), true) = (&yuv, ffcb.is_some()) {
                    let want = &yuv[n as usize * fsize..];
                    for r in 0..h {
                        assert_eq!(&y[r * pw..r * pw + w], &want[r * w..(r + 1) * w], "{name} frame {n} row {r}");
                    }
                    let (cw, q) = (w / 4, (w / 4) * (h / 4));
                    for r in 0..h / 4 {
                        assert_eq!(&cb[r * pw / 4..r * pw / 4 + cw], &want[w * h + r * cw..][..cw], "{name} frame {n} Cb row {r}");
                        assert_eq!(&cr[r * pw / 4..r * pw / 4 + cw], &want[w * h + q + r * cw..][..cw], "{name} frame {n} Cr row {r}");
                    }
                }
            }
            eprintln!("{name}: {} frames {w}x{h}; IBM codebook vs ffmpeg's: mean luma difference {:.3}, max {max_ibm}; total variation ffmpeg's {} IBM's {}", a.frame_count(), diff_ibm as f64 / px.max(1) as f64, tv.0, tv.1);
        }
    }
}
