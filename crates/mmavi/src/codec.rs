//! Video codecs: uncompressed DIBs, RLE8, Microsoft Video 1 and IBM
//! Ultimotion, each decoding a frame chunk into the previous frame.

use mmimage::Image;

use crate::ulti::Ulti;
use crate::{Avi, Error, le16};

enum Codec {
    /// BI_RGB with this many bits per pixel, rows bottom-up unless
    /// `top_down`.
    Dib { bits: u16, top_down: bool },
    Rle8,
    /// Microsoft Video 1, 8 or 16 bits.
    Cram8,
    Cram16,
    Ulti(Ulti),
}

/// Decodes the video stream frame by frame. [`image`](Self::image) is the
/// current frame, rows top-down: 8-bit with the stream's palette for
/// palettized codecs, 24-bit otherwise.
pub struct VideoDecoder {
    codec: Codec,
    img: Image,
}

impl VideoDecoder {
    pub fn new(avi: &Avi) -> Result<VideoDecoder, Error> {
        let f = avi.video_format();
        let (w, h) = (avi.width(), avi.height());
        if w == 0 || h == 0 || w > 8192 || h > 8192 { return Err(Error::Corrupt); }
        let handler = avi.video_stream.map(|v| v.1.handler).unwrap_or([0; 4]);
        let codec = match (&f.fourcc(), f.bit_count) {
            (b"\0\0\0\0", 8 | 16 | 24 | 32) => Codec::Dib { bits: f.bit_count, top_down: f.height < 0 },
            (b"DIB ", 8 | 16 | 24 | 32) | (b"RGB ", 8 | 16 | 24 | 32) => Codec::Dib { bits: f.bit_count, top_down: false },
            ([1, 0, 0, 0], 8) | (b"mrle", 8) | (b"RLE8", 8) | (b"RLE ", 8) => Codec::Rle8,
            (b"CRAM" | b"MSVC" | b"msvc" | b"cram", 8) => Codec::Cram8,
            (b"CRAM" | b"MSVC" | b"msvc" | b"cram", 15 | 16) => Codec::Cram16,
            (b"ULTI" | b"ulti", _) => Codec::Ulti(Ulti::new(w, h)),
            _ if &handler == b"ULTI" => Codec::Ulti(Ulti::new(w, h)),
            _ => return Err(Error::Unsupported),
        };
        let pal8 = matches!(codec, Codec::Rle8 | Codec::Cram8 | Codec::Dib { bits: 8, .. });
        let mut img = Image::new(w, h, if pal8 { 8 } else { 24 });
        if pal8 {
            img.palette = f.palette.clone();
            img.palette.resize(256, [0; 3]);
        }
        Ok(VideoDecoder { codec, img })
    }

    /// The current frame.
    pub fn image(&self) -> &Image { &self.img }

    /// The Ultimotion state, for setting its codebook.
    pub fn ulti(&mut self) -> Option<&mut Ulti> {
        match &mut self.codec { Codec::Ulti(u) => Some(u), _ => None }
    }

    /// Applies a palette change chunk (AVIPALCHANGE: first entry, count
    /// (0 = 256), flags, then R,G,B,flags entries).
    pub fn palette_change(&mut self, chunk: &[u8]) {
        let (Some(&first), Some(&n)) = (chunk.first(), chunk.get(1)) else { return };
        let n = if n == 0 { 256 } else { n as usize };
        if self.img.palette.is_empty() { return; }
        for i in 0..n {
            let Some(e) = chunk.get(4 + i * 4..7 + i * 4) else { break };
            let slot = first as usize + i;
            if let Some(p) = self.img.palette.get_mut(slot) { *p = [e[0], e[1], e[2]]; }
        }
    }

    /// Decodes the next frame chunk over the previous frame (an empty
    /// chunk repeats it).
    pub fn decode(&mut self, chunk: &[u8]) -> Result<(), Error> {
        if chunk.is_empty() { return Ok(()); }
        let img = &mut self.img;
        let (w, h) = (img.width, img.height);
        match &mut self.codec {
            Codec::Dib { bits, top_down } => {
                let srb = (w as usize * *bits as usize).div_ceil(32) * 4;
                if chunk.len() < srb { return Err(Error::Truncated); }
                for sy in 0..h {
                    let Some(row) = chunk.get(sy as usize * srb..(sy as usize + 1) * srb) else { break };
                    let y = if *top_down { sy } else { h - 1 - sy };
                    match bits {
                        8 => img.row_mut(y)[..w as usize].copy_from_slice(&row[..w as usize]),
                        24 => img.row_mut(y)[..w as usize * 3].copy_from_slice(&row[..w as usize * 3]),
                        32 => for x in 0..w as usize { img.set_rgb(x as u32, y, [row[x * 4 + 2], row[x * 4 + 1], row[x * 4]]) },
                        _ => for x in 0..w { img.set_rgb(x, y, rgb555(le16(row, x as usize * 2).unwrap_or(0))) },
                    }
                }
            }
            Codec::Rle8 => mmimage::bmp::rle_decode(chunk, img, 1, true).map_err(|_| Error::Corrupt)?,
            Codec::Cram8 | Codec::Cram16 => {
                let deep = matches!(self.codec, Codec::Cram16);
                cram(chunk, img, deep)?;
            }
            Codec::Ulti(u) => {
                u.decode(chunk)?;
                u.to_rgb(img);
            }
        }
        Ok(())
    }
}

fn rgb555(v: u16) -> [u8; 3] {
    let s = |x: u16| { let x = (x & 31) as u8; x << 3 | x >> 2 };
    [s(v >> 10), s(v >> 5), s(v)]
}

/// Microsoft Video 1. The frame is 4×4 blocks sent bottom-up (as DIB
/// rows), left to right; each starts with a 16-bit little-endian word:
///
/// - 0x84xx..0x87xx: skip ((hi - 0x84) << 8 | lo) blocks, this one
///   included (they keep the previous frame);
/// - below 0x8000: a 16-bit mask (bit 0 = bottom-left pixel, rows
///   bottom-up, set = first colour) with two colours, or, when bit 15 of
///   the first 16-bit colour is set (or the high byte is 0x90 or more
///   for 8-bit), eight colours: one pair per 2×2 quadrant (bottom-left,
///   bottom-right, top-left, top-right);
/// - otherwise one colour for the whole block (the low byte for 8-bit,
///   the word itself for 16-bit).
fn cram(d: &[u8], img: &mut Image, deep: bool) -> Result<(), Error> {
    let (bw, bh) = (img.width / 4, img.height / 4);
    let mut i = 0;
    let mut skip = 0u32;
    let byte = |i: &mut usize| -> Result<u8, Error> { let b = *d.get(*i).ok_or(Error::Truncated)?; *i += 1; Ok(b) };
    let colour = |i: &mut usize| -> Result<u16, Error> { let v = le16(d, *i).ok_or(Error::Truncated)?; *i += 2; Ok(v) };
    for by in (0..bh).rev() {
        for bx in 0..bw {
            if skip > 0 { skip -= 1; continue; }
            let (Some(&lo), Some(&hi)) = (d.get(i), d.get(i + 1)) else { return Ok(()) };
            i += 2;
            let flags = (hi as u16) << 8 | lo as u16;
            if hi & 0xFC == 0x84 {
                skip = (((hi - 0x84) as u32) << 8 | lo as u32).saturating_sub(1);
                continue;
            }
            // Eight colours (or two) in quadrant order, as palette
            // indexes or RGB555 words.
            let mut c = [0u16; 8];
            let quads;
            if hi < 0x80 {
                if deep {
                    c[0] = colour(&mut i)?;
                    c[1] = colour(&mut i)?;
                    quads = c[0] & 0x8000 != 0;
                    if quads { for k in 2..8 { c[k] = colour(&mut i)?; } }
                } else {
                    c[0] = byte(&mut i)? as u16;
                    c[1] = byte(&mut i)? as u16;
                    quads = false;
                }
            } else if !deep && hi >= 0x90 {
                for k in 0..8 { c[k] = byte(&mut i)? as u16; }
                quads = true;
            } else {
                let v = if deep { flags } else { lo as u16 };
                c = [v; 8];
                quads = false;
            }
            let (bits, all) = if hi < 0x80 || quads { (flags, false) } else { (0xFFFF, true) };
            for py in 0..4u32 {
                for px in 0..4u32 {
                    let k = py * 4 + px;
                    let set = all || (bits >> k) & 1 != 0;
                    let q = if quads { ((py & 2) << 1 | (px & 2)) as usize } else { 0 };
                    let v = c[q + if set { 0 } else { 1 }];
                    let (x, y) = (bx * 4 + px, by * 4 + 3 - py);
                    if deep { img.set_rgb(x, y, rgb555(v & 0x7FFF)) } else { img.set_index(x, y, v as u8) }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;
    use alloc::vec;

    fn decoder(w: i32, h: i32, bits: u16, comp: &[u8; 4], pal: &[[u8; 3]], frames: &[Vec<u8>]) -> (Avi, VideoDecoder, Vec<u8>) {
        let f = build(w, h, bits, comp, pal, frames, &[], true);
        let a = Avi::parse(&f).unwrap();
        let d = VideoDecoder::new(&a).unwrap();
        (a, d, f)
    }

    fn frame<'a>(a: &Avi, f: &'a [u8], n: u32) -> &'a [u8] {
        let (o, l) = a.frame_chunk(n).unwrap();
        &f[o as usize..o as usize + l as usize]
    }

    #[test]
    fn dib_frames() {
        // 2x2 24-bit, bottom-up.
        let (a, mut d, f) = decoder(2, 2, 24, b"\0\0\0\0", &[], &[vec![1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10, 11, 12, 0, 0]]);
        d.decode(frame(&a, &f, 0)).unwrap();
        assert_eq!(d.image().rgb(0, 1), [3, 2, 1]);
        assert_eq!(d.image().rgb(1, 0), [12, 11, 10]);
        let (a, mut d, f) = decoder(2, 1, 16, b"\0\0\0\0", &[], &[vec![0x00, 0x7C, 0x1F, 0x00]]);
        d.decode(frame(&a, &f, 0)).unwrap();
        assert_eq!(d.image().rgb(0, 0), [255, 0, 0]);
        assert_eq!(d.image().rgb(1, 0), [0, 0, 255]);
        let (a, mut d, f) = decoder(1, 1, 32, b"\0\0\0\0", &[], &[vec![1, 2, 3, 0]]);
        d.decode(frame(&a, &f, 0)).unwrap();
        assert_eq!(d.image().rgb(0, 0), [3, 2, 1]);
        assert!(d.decode(&[1]).is_err());
    }

    #[test]
    fn rle8_delta() {
        let pal = [[0, 0, 0], [255, 0, 0], [0, 255, 0]];
        // 4x2: key frame all 1, then a delta changing one pixel (2,0 top).
        let key = vec![4, 1, 0, 0, 4, 1, 0, 1];
        let delta = vec![0, 2, 2, 1, 1, 2, 0, 1];
        let (a, mut d, f) = decoder(4, 2, 8, &[1, 0, 0, 0], &pal, &[key, delta]);
        d.decode(frame(&a, &f, 0)).unwrap();
        assert!((0..4).all(|x| d.image().index(x, 0) == 1 && d.image().index(x, 1) == 1));
        d.decode(frame(&a, &f, 1)).unwrap();
        assert_eq!(d.image().index(2, 0), 2);
        assert_eq!(d.image().index(1, 0), 1);
    }

    #[test]
    fn cram8_blocks() {
        let pal: Vec<[u8; 3]> = (0..=255u8).map(|v| [v; 3]).collect();
        // 8x4: block 0 two-colour (mask 0x0001: only bottom-left pixel
        // first colour), block 1 one colour 7.
        let fr = vec![0x01, 0x00, 5, 6, 7, 0x80];
        let (a, mut d, f) = decoder(8, 4, 8, b"CRAM", &pal, &[fr, vec![0x02, 0x84]]);
        d.decode(frame(&a, &f, 0)).unwrap();
        let im = d.image();
        assert_eq!(im.index(0, 3), 5);
        assert_eq!(im.index(1, 3), 6);
        assert_eq!(im.index(0, 0), 6);
        assert!((4..8).all(|x| (0..4).all(|y| im.index(x, y) == 7)));
        // A skip of both blocks keeps the frame.
        let before = d.image().clone();
        d.decode(frame(&a, &f, 1)).unwrap();
        assert_eq!(&before, d.image());
    }

    #[test]
    fn unsupported() {
        let f = build(4, 4, 24, b"IV31", &[], &[vec![0; 4]], &[], true);
        assert_eq!(VideoDecoder::new(&Avi::parse(&f).unwrap()).err(), Some(Error::Unsupported));
    }

    /// Runs ffmpeg (when installed) to decode `avi` to raw frames.
    pub fn ffmpeg(avi: &std::path::Path, pix: &str) -> Option<Vec<u8>> {
        let o = std::process::Command::new("ffmpeg").args(["-loglevel", "error", "-i"]).arg(avi)
            .args(["-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", pix, "-"]).output().ok()?;
        o.status.success().then_some(o.stdout)
    }

    #[test]
    fn ffmpeg_encoded() {
        // Files written by ffmpeg's encoders, decoded by us and by ffmpeg.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mmavi-test");
        let _ = std::fs::create_dir_all(&dir);
        for (name, args, tol) in [
            ("msvc16", &["-c:v", "msvideo1"][..], 0),
            ("msrle", &["-pix_fmt", "pal8", "-c:v", "msrle"][..], 0),
            ("rgb24", &["-pix_fmt", "bgr24", "-c:v", "rawvideo"][..], 0),
            ("rgb555", &["-pix_fmt", "rgb555le", "-c:v", "rawvideo"][..], 0),
            ("pal8", &["-pix_fmt", "pal8", "-c:v", "rawvideo"][..], 0),
        ] {
            let p = dir.join(format!("{name}.avi"));
            let ok = std::process::Command::new("ffmpeg").args(["-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=68x52:rate=5", "-t", "3"])
                .args(args).arg(&p).status().is_ok_and(|s| s.success());
            if !ok { return; }
            let data = std::fs::read(&p).unwrap();
            let a = Avi::parse(&data).unwrap();
            let mut d = VideoDecoder::new(&a).unwrap_or_else(|e| panic!("{name}: {e:?} {:?}", a.video_format()));
            let Some(raw) = ffmpeg(&p, "rgb24") else { return };
            let (w, h) = (a.width(), a.height());
            assert_eq!(a.frame_count() as usize, raw.len() / (w * h * 3) as usize, "{name}");
            for n in 0..a.frame_count() {
                let (o, l) = a.frame_chunk(n).unwrap();
                d.decode(&data[o as usize..o as usize + l as usize]).unwrap();
                for y in 0..h {
                    for x in 0..w {
                        let at = (((n * h + y) * w + x) * 3) as usize;
                        let (got, want) = (d.image().rgb(x, y), &raw[at..at + 3]);
                        assert!((0..3).all(|k| (got[k] as i32 - want[k] as i32).abs() <= tol), "{name} frame {n} at {x},{y}: {got:?} vs {want:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn lexps_against_ffmpeg() {
        // IBM's CRAM 8-bit sample against ffmpeg's decoder (indexes).
        let Some(data) = work("media/client/os2image/pmdd_6/LEXPS.AVI") else { return };
        let a = Avi::parse(&data).unwrap();
        let mut d = VideoDecoder::new(&a).unwrap();
        let path = work_path("media/client/os2image/pmdd_6/LEXPS.AVI");
        let Some(raw) = ffmpeg(&path, "pal8") else { return };
        let (w, h) = (a.width() as usize, a.height() as usize);
        let fsize = w * h + 1024;
        assert_eq!(raw.len() / fsize, a.frame_count() as usize);
        for n in 0..a.frame_count() {
            let (o, l) = a.frame_chunk(n).unwrap();
            d.decode(&data[o as usize..o as usize + l as usize]).unwrap();
            let want = &raw[n as usize * fsize..][..w * h];
            for y in 0..h {
                assert_eq!(&d.image().row(y as u32)[..w], &want[y * w..(y + 1) * w], "frame {n} row {y}");
            }
        }
    }
}
