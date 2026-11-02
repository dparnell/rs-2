//! AVI files for rs2's digital video device (MDM) and AVI I/O procedure,
//! with no OS dependencies (host-tested).
//!
//! An AVI is a RIFF form "AVI ": LIST "hdrl" (the main header "avih" and
//! one LIST "strl" per stream: header "strh", format "strf", name
//! "strn"), LIST "movi" (the data chunks, possibly grouped in LIST "rec "
//! records) and the index "idx1". Data chunk IDs are the stream number
//! in two decimal digits and a type: "db"/"dc" video frames, "pc" palette
//! changes, "wb" audio. OpenDML (AVI 2.0) files continue in further RIFF
//! "AVIX" forms, each with its own LIST "movi"; these are found by
//! scanning the chunks (their ix## indexes are not needed then).
//!
//! [`Avi::parse`] reads the whole file or a prefix of it (enough for the
//! headers; [`Avi::index_offset`] then says where idx1 is and
//! [`Avi::load_index`] takes it). [`VideoDecoder`] decodes the video
//! stream: uncompressed DIBs, RLE8, Microsoft Video 1 ("CRAM") and IBM
//! Ultimotion ("ULTI"; see [`ulti`]).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;

mod codec;
pub mod ulti;

pub use codec::VideoDecoder;
pub use mmimage::Image;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not a RIFF AVI file.
    NotAvi,
    /// The data ends before the headers (or a frame) do.
    Truncated,
    /// A codec or format that is not implemented.
    Unsupported,
    /// Inconsistent data.
    Corrupt,
}

/// The video stream's format (its BITMAPINFOHEADER).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VideoFormat {
    pub width: i32,
    /// Negative for top-down DIBs.
    pub height: i32,
    pub bit_count: u16,
    /// biCompression: 0 = BI_RGB, 1 = BI_RLE8, 2 = BI_RLE4, 3 =
    /// BI_BITFIELDS, otherwise a FOURCC.
    pub compression: u32,
    /// The colour table (R,G,B).
    pub palette: Vec<[u8; 3]>,
}

impl VideoFormat {
    /// biCompression as four bytes ("\0\0\0\0" for BI_RGB).
    pub fn fourcc(&self) -> [u8; 4] { self.compression.to_le_bytes() }
}

/// The audio stream's format (its WAVEFORMATEX).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioFormat {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub avg_bytes: u32,
    pub block_align: u16,
    pub bits: u16,
    /// Bytes after cbSize (codec specific).
    pub extra: Vec<u8>,
}

/// A stream header (strh).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamHeader {
    pub kind: [u8; 4],
    pub handler: [u8; 4],
    pub flags: u32,
    pub initial_frames: u32,
    pub scale: u32,
    pub rate: u32,
    pub start: u32,
    pub length: u32,
    pub suggested_buffer: u32,
    pub sample_size: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Avi {
    pub usec_per_frame: u32,
    pub total_frames: u32,
    pub streams: u32,
    pub width: u32,
    pub height: u32,
    /// The first video and audio streams: stream number and header.
    pub video_stream: Option<(u32, StreamHeader)>,
    pub audio_stream: Option<(u32, StreamHeader)>,
    video_format: VideoFormat,
    audio_format: Option<AudioFormat>,
    /// Video frame chunks (offset of the data in the file, length);
    /// zero-length entries repeat the previous frame.
    frames: Vec<(u64, u32)>,
    /// Key frame flags from idx1 (empty when scanned).
    keyframes: Vec<bool>,
    /// Palette changes: (index of the frame they precede, offset, len).
    palette_changes: Vec<(u32, u64, u32)>,
    audio: Vec<(u64, u32)>,
    /// File offset of the "movi" FOURCC (idx1 offsets count from it).
    movi: u64,
    /// File offset of the idx1 chunk header, if known.
    idx1: Option<u64>,
    indexed: bool,
}

fn le16(d: &[u8], at: usize) -> Option<u16> { Some(u16::from_le_bytes(d.get(at..at.checked_add(2)?)?.try_into().ok()?)) }
fn le32(d: &[u8], at: usize) -> Option<u32> { Some(u32::from_le_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?)) }

/// Stream number and two-letter type of a data chunk ID ("01wb").
fn chunk_stream(id: &[u8]) -> Option<(u32, [u8; 2])> {
    let (a, b) = (id[0].wrapping_sub(b'0'), id[1].wrapping_sub(b'0'));
    if a > 9 || b > 9 { return None; }
    Some(((a * 10 + b) as u32, [id[2], id[3]]))
}

impl Avi {
    /// Parses an AVI from the whole file, or from a prefix holding at
    /// least the headers.
    pub fn parse(data: &[u8]) -> Result<Avi, Error> {
        if data.len() < 12 { return Err(if data.starts_with(b"RIFF") || data.is_empty() { Error::Truncated } else { Error::NotAvi }); }
        if &data[..4] != b"RIFF" || &data[8..12] != b"AVI " { return Err(Error::NotAvi); }
        let mut a = Avi::default();
        let riff_end = (8 + le32(data, 4).unwrap_or(0) as u64).min(data.len() as u64) as usize;
        let mut have_hdrl = false;
        let mut idx1_data: Option<(usize, usize)> = None;
        let mut movis: Vec<(usize, usize)> = Vec::new();
        let mut at = 12;
        let mut end = riff_end;
        let mut avix = false;
        loop {
            while at + 8 <= end {
                let id = &data[at..at + 4];
                let len = le32(data, at + 4).unwrap_or(0) as usize;
                let body = at + 8;
                let body_end = body.saturating_add(len).min(data.len());
                if id == b"LIST" && body + 4 <= data.len() {
                    match &data[body..body + 4] {
                        b"hdrl" => {
                            if body_end < body + len { return Err(Error::Truncated); }
                            a.parse_hdrl(&data[body + 4..body_end])?;
                            have_hdrl = true;
                        }
                        b"movi" => {
                            if movis.is_empty() { a.movi = body as u64; a.idx1 = Some(body.saturating_add(len + (len & 1)) as u64); }
                            movis.push((body + 4, body_end));
                        }
                        _ => {}
                    }
                } else if id == b"idx1" {
                    a.idx1 = Some(at as u64);
                    if body_end == body + len { idx1_data = Some((body, body_end)); }
                }
                at = body.saturating_add(len + (len & 1));
            }
            // OpenDML continuation forms.
            if at + 12 <= data.len() && &data[at..at + 4] == b"RIFF" && &data[at + 8..at + 12] == b"AVIX" {
                avix = true;
                end = (at + 8 + le32(data, at + 4).unwrap_or(0) as usize).min(data.len());
                at += 12;
                continue;
            }
            break;
        }
        if !have_hdrl { return Err(Error::Truncated); }
        match idx1_data {
            Some((s, e)) if !avix => a.load_index(&data[s..e]),
            _ => {
                for (s, e) in movis { a.scan(data, s, e); }
            }
        }
        Ok(a)
    }

    fn parse_hdrl(&mut self, d: &[u8]) -> Result<(), Error> {
        let mut at = 0;
        let mut stream = 0u32;
        while at + 8 <= d.len() {
            let id = &d[at..at + 4];
            let len = le32(d, at + 4).unwrap_or(0) as usize;
            let body = &d[at + 8..(at + 8).saturating_add(len).min(d.len())];
            if id == b"avih" {
                let f = |i: usize| le32(body, i * 4).unwrap_or(0);
                self.usec_per_frame = f(0);
                self.total_frames = f(4);
                self.streams = f(6);
                self.width = f(8);
                self.height = f(9);
            } else if id == b"LIST" && body.starts_with(b"strl") {
                self.parse_strl(stream, &body[4..])?;
                stream += 1;
            }
            at = (at + 8).saturating_add(len + (len & 1));
        }
        Ok(())
    }

    fn parse_strl(&mut self, n: u32, d: &[u8]) -> Result<(), Error> {
        let mut at = 0;
        let mut hdr: Option<StreamHeader> = None;
        while at + 8 <= d.len() {
            let id = &d[at..at + 4];
            let len = le32(d, at + 4).unwrap_or(0) as usize;
            let body = &d[at + 8..(at + 8).saturating_add(len).min(d.len())];
            match id {
                b"strh" if body.len() >= 36 => {
                    let f = |i: usize| le32(body, i).unwrap_or(0);
                    hdr = Some(StreamHeader {
                        kind: body[0..4].try_into().unwrap_or([0; 4]),
                        handler: body[4..8].try_into().unwrap_or([0; 4]),
                        flags: f(8),
                        initial_frames: f(16),
                        scale: f(20),
                        rate: f(24),
                        start: f(28),
                        length: f(32),
                        suggested_buffer: f(36),
                        sample_size: f(44),
                    });
                }
                b"strf" => match hdr {
                    Some(h) if &h.kind == b"vids" && self.video_stream.is_none() => {
                        self.video_stream = Some((n, h));
                        self.video_format = parse_bih(body)?;
                    }
                    Some(h) if &h.kind == b"auds" && self.audio_stream.is_none() => {
                        self.audio_stream = Some((n, h));
                        let g = |i| le16(body, i).unwrap_or(0);
                        let cb = g(16) as usize;
                        self.audio_format = Some(AudioFormat {
                            tag: g(0),
                            channels: g(2),
                            rate: le32(body, 4).unwrap_or(0),
                            avg_bytes: le32(body, 8).unwrap_or(0),
                            block_align: g(12),
                            bits: g(14),
                            extra: body.get(18..18 + cb).unwrap_or(&[]).to_vec(),
                        });
                    }
                    _ => {}
                },
                _ => {}
            }
            at = (at + 8).saturating_add(len + (len & 1));
        }
        Ok(())
    }

    /// Takes the body of the idx1 chunk (16-byte entries: ID, flags,
    /// offset, size) when it was read separately from the headers.
    pub fn load_index(&mut self, d: &[u8]) {
        self.frames.clear();
        self.keyframes.clear();
        self.palette_changes.clear();
        self.audio.clear();
        // Offsets count from the "movi" FOURCC, or (some writers) from
        // the start of the file: an offset smaller than movi's means the
        // former.
        let first = d.chunks_exact(16).next().and_then(|e| le32(e, 8)).unwrap_or(0) as u64;
        let base = if first >= self.movi && self.movi > 0 { 0 } else { self.movi };
        for e in d.chunks_exact(16) {
            let flags = le32(e, 4).unwrap_or(0);
            let off = base + le32(e, 8).unwrap_or(0) as u64 + 8;
            let len = le32(e, 12).unwrap_or(0);
            if &e[..4] == b"rec " || &e[..4] == b"LIST" { continue; }
            self.add(&e[..4], off, len, flags & 0x10 != 0);
        }
        self.indexed = true;
    }

    fn add(&mut self, id: &[u8], off: u64, len: u32, key: bool) {
        let Some((n, ty)) = chunk_stream(id) else { return };
        if Some(n) == self.video_stream.map(|v| v.0) {
            match &ty {
                b"db" | b"dc" | b"iv" => { self.frames.push((off, len)); self.keyframes.push(key); }
                b"pc" => self.palette_changes.push((self.frames.len() as u32, off, len)),
                _ => {}
            }
        } else if Some(n) == self.audio_stream.map(|v| v.0) && &ty == b"wb" {
            self.audio.push((off, len));
        }
    }

    /// Lists the data chunks of a movi list (descending into "rec ").
    fn scan(&mut self, data: &[u8], mut at: usize, end: usize) {
        while at + 8 <= end {
            let id = &data[at..at + 4];
            let len = le32(data, at + 4).unwrap_or(0) as usize;
            if id == b"LIST" {
                // Records hold chunks; step inside.
                at += 12;
                continue;
            }
            // A chunk cut off by the end of a prefix still counts when its
            // header is complete: the caller reads it from the file.
            if id != b"JUNK" && !id.starts_with(b"ix") { self.add(id, at as u64 + 8, len as u32, false); }
            at = (at + 8).saturating_add(len + (len & 1));
        }
    }

    /// The file offset of the idx1 chunk (its header), when known.
    pub fn index_offset(&self) -> Option<u64> { self.idx1 }

    /// Whether the frame list came from idx1.
    pub fn indexed(&self) -> bool { self.indexed }

    pub fn width(&self) -> u32 { if self.video_format.width > 0 { self.video_format.width as u32 } else { self.width } }
    pub fn height(&self) -> u32 { if self.video_format.height != 0 { self.video_format.height.unsigned_abs() } else { self.height } }

    /// Frames in the video stream: as found, or as the headers say.
    pub fn frame_count(&self) -> u32 {
        if !self.frames.is_empty() { return self.frames.len() as u32; }
        self.video_stream.map(|v| v.1.length).filter(|&l| l > 0).unwrap_or(self.total_frames)
    }

    /// Microseconds per frame, from the video stream's rate (or avih).
    pub fn usec_per_frame(&self) -> u32 {
        match self.video_stream {
            Some((_, h)) if h.rate > 0 && h.scale > 0 => ((h.scale as u64 * 1_000_000 + h.rate as u64 / 2) / h.rate as u64) as u32,
            _ => self.usec_per_frame,
        }
    }

    /// The video compression FOURCC (biCompression; the stream handler
    /// when that is BI_RGB/BI_RLE8 and the handler is a FOURCC).
    pub fn video_fourcc(&self) -> [u8; 4] { self.video_format.fourcc() }

    pub fn video_format(&self) -> &VideoFormat { &self.video_format }

    /// Frame `n`'s data chunk: (offset of the data in the file, length).
    pub fn frame_chunk(&self, n: u32) -> Option<(u64, u32)> { self.frames.get(n as usize).copied() }

    /// Whether frame `n` is marked a key frame in idx1 (always false for
    /// scanned files).
    pub fn is_keyframe(&self, n: u32) -> bool { self.keyframes.get(n as usize).copied().unwrap_or(false) }

    /// Palette change chunks: (index of the frame they apply before,
    /// offset, length); feed them to [`VideoDecoder::palette_change`].
    pub fn palette_changes(&self) -> &[(u32, u64, u32)] { &self.palette_changes }

    pub fn audio_format(&self) -> Option<&AudioFormat> { self.audio_format.as_ref() }

    /// The audio stream's data chunks in order: (offset, length).
    pub fn audio_chunks(&self) -> &[(u64, u32)] { &self.audio }
}

/// Parses a BITMAPINFOHEADER (or OS/2 1.x BITMAPCOREHEADER) and its
/// colour table.
fn parse_bih(d: &[u8]) -> Result<VideoFormat, Error> {
    let size = le32(d, 0).ok_or(Error::Truncated)? as usize;
    let mut f = VideoFormat::default();
    let (entry, count);
    if size == 12 {
        f.width = le16(d, 4).unwrap_or(0) as i32;
        f.height = le16(d, 6).unwrap_or(0) as i32;
        f.bit_count = le16(d, 10).unwrap_or(0);
        entry = 3;
        count = if f.bit_count <= 8 { 1usize << f.bit_count } else { 0 };
    } else {
        if d.len() < 20 { return Err(Error::Truncated); }
        f.width = le32(d, 4).unwrap_or(0) as i32;
        f.height = le32(d, 8).unwrap_or(0) as i32;
        f.bit_count = le16(d, 14).unwrap_or(0);
        f.compression = le32(d, 16).unwrap_or(0);
        let used = le32(d, 32).unwrap_or(0) as usize;
        entry = 4;
        count = if used > 0 && used <= 256 { used } else if f.bit_count <= 8 { 1usize << f.bit_count } else { 0 };
    }
    for i in 0..count {
        let at = size.max(12) + i * entry;
        let Some(p) = d.get(at..at + 3) else { break };
        f.palette.push([p[2], p[1], p[0]]);
    }
    Ok(f)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alloc::vec;

    pub fn chunk(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut o = id.to_vec();
        o.extend_from_slice(&(body.len() as u32).to_le_bytes());
        o.extend_from_slice(body);
        if body.len() % 2 == 1 { o.push(0); }
        o
    }

    pub fn list(kind: &[u8], body: &[u8]) -> Vec<u8> {
        let mut b = kind.to_vec();
        b.extend_from_slice(body);
        chunk(b"LIST", &b)
    }

    /// A small AVI: video `fourcc`/`bits` frames and optional audio, with
    /// or without idx1.
    pub fn build(w: i32, h: i32, bits: u16, comp: &[u8; 4], palette: &[[u8; 3]], frames: &[Vec<u8>], audio: &[Vec<u8>], index: bool) -> Vec<u8> {
        let mut avih = vec![0u8; 56];
        avih[0..4].copy_from_slice(&66666u32.to_le_bytes());
        avih[16..20].copy_from_slice(&(frames.len() as u32).to_le_bytes());
        avih[24..28].copy_from_slice(&(1 + !audio.is_empty() as u32).to_le_bytes());
        avih[32..36].copy_from_slice(&(w as u32).to_le_bytes());
        avih[36..40].copy_from_slice(&(h.unsigned_abs()).to_le_bytes());
        let mut strh = vec![0u8; 56];
        strh[0..4].copy_from_slice(b"vids");
        strh[4..8].copy_from_slice(comp);
        strh[20..24].copy_from_slice(&1u32.to_le_bytes());
        strh[24..28].copy_from_slice(&15u32.to_le_bytes());
        strh[32..36].copy_from_slice(&(frames.len() as u32).to_le_bytes());
        let mut strf = vec![0u8; 40];
        strf[0..4].copy_from_slice(&40u32.to_le_bytes());
        strf[4..8].copy_from_slice(&w.to_le_bytes());
        strf[8..12].copy_from_slice(&h.to_le_bytes());
        strf[12..14].copy_from_slice(&1u16.to_le_bytes());
        strf[14..16].copy_from_slice(&bits.to_le_bytes());
        strf[16..20].copy_from_slice(comp);
        strf[32..36].copy_from_slice(&(palette.len() as u32).to_le_bytes());
        for c in palette { strf.extend_from_slice(&[c[2], c[1], c[0], 0]); }
        let mut strls = list(b"strl", &[chunk(b"strh", &strh), chunk(b"strf", &strf)].concat());
        if !audio.is_empty() {
            let mut ah = vec![0u8; 56];
            ah[0..4].copy_from_slice(b"auds");
            ah[20..24].copy_from_slice(&1u32.to_le_bytes());
            ah[24..28].copy_from_slice(&11025u32.to_le_bytes());
            let mut wf = vec![];
            for v in [1u16, 1] { wf.extend_from_slice(&v.to_le_bytes()); }
            wf.extend_from_slice(&11025u32.to_le_bytes());
            wf.extend_from_slice(&11025u32.to_le_bytes());
            for v in [1u16, 8, 0] { wf.extend_from_slice(&v.to_le_bytes()); }
            strls.extend(list(b"strl", &[chunk(b"strh", &ah), chunk(b"strf", &wf)].concat()));
        }
        let hdrl = list(b"hdrl", &[chunk(b"avih", &avih), strls].concat());
        let mut movi = Vec::new();
        let mut idx = Vec::new();
        let mut put = |id: &[u8], body: &[u8], movi: &mut Vec<u8>| {
            idx.extend_from_slice(id);
            idx.extend_from_slice(&0x10u32.to_le_bytes());
            idx.extend_from_slice(&(movi.len() as u32 + 4).to_le_bytes());
            idx.extend_from_slice(&(body.len() as u32).to_le_bytes());
            movi.extend(chunk(id, body));
        };
        for (i, f) in frames.iter().enumerate() {
            put(if comp == b"\0\0\0\0" { b"00db" } else { b"00dc" }, f, &mut movi);
            if let Some(a) = audio.get(i) { put(b"01wb", a, &mut movi); }
        }
        let mut body = b"AVI ".to_vec();
        body.extend(hdrl);
        body.extend(list(b"movi", &movi));
        if index { body.extend(chunk(b"idx1", &idx)); }
        chunk(b"RIFF", &body)
    }

    #[test]
    fn parse_small() {
        let pal = [[1, 2, 3], [4, 5, 6]];
        let f = build(4, 2, 8, b"\0\0\0\0", &pal, &[vec![0; 8], vec![1; 8]], &[vec![9; 5], vec![7; 3]], true);
        for (n, data) in [("indexed", f.clone()), ("scanned", build(4, 2, 8, b"\0\0\0\0", &pal, &[vec![0; 8], vec![1; 8]], &[vec![9; 5], vec![7; 3]], false))] {
            let a = Avi::parse(&data).unwrap();
            assert_eq!(a.indexed(), n == "indexed");
            assert_eq!((a.width(), a.height(), a.frame_count()), (4, 2, 2), "{n}");
            assert_eq!(a.usec_per_frame(), 66667);
            assert_eq!(a.video_format().palette, pal);
            let (off, len) = a.frame_chunk(1).unwrap();
            assert_eq!(&data[off as usize..off as usize + len as usize], &[1; 8]);
            assert_eq!(a.audio_chunks().len(), 2);
            let (off, len) = a.audio_chunks()[0];
            assert_eq!(&data[off as usize..off as usize + len as usize], &[9; 5]);
            assert_eq!(a.audio_format().unwrap().rate, 11025);
        }
        // Headers only, then the index read separately.
        let full = Avi::parse(&f).unwrap();
        let ix = full.index_offset().unwrap() as usize;
        let movi = f.windows(4).position(|w| w == b"movi").unwrap();
        assert_eq!(Avi::parse(&f[..movi - 20]).err(), Some(Error::Truncated));
        let mut a = Avi::parse(&f[..movi + 4]).unwrap();
        assert_eq!(a.index_offset(), Some(ix as u64));
        a.load_index(&f[ix + 8..]);
        assert_eq!(a.frame_chunk(1), full.frame_chunk(1));
        // Garbage never panics.
        for n in 0..f.len() { let _ = Avi::parse(&f[..n]); }
        assert_eq!(Avi::parse(b"RIFF\0\0\0\0WAVEfmt ").err(), Some(Error::NotAvi));
    }

    /// rs2's work directory (IBM's media), when present.
    pub fn work_path(path: &str) -> std::path::PathBuf {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../../work");
        let root = if root.exists() { root } else { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work") };
        root.join(path)
    }

    pub fn work(path: &str) -> Option<Vec<u8>> { std::fs::read(work_path(path)).ok() }

    /// MACAW.AVI comes split over two install files.
    pub fn macaw() -> Option<Vec<u8>> {
        let mut a = work("unpacked/disk_10/MMTEMP/MACAW.AV1")?;
        a.extend(work("unpacked/disk_10/MMTEMP/MACAW.AV2")?);
        Some(a)
    }

    #[test]
    fn ibm_files() {
        if let Some(m) = macaw() {
            let a = Avi::parse(&m).unwrap();
            assert_eq!(&a.video_fourcc(), b"ULTI");
            assert_eq!((a.width(), a.height()), (320, 240));
            assert_eq!(a.frame_count(), 225);
            let af = a.audio_format().unwrap();
            assert_eq!((af.tag, af.channels, af.rate, af.bits), (1, 1, 22050, 8));
            assert!(!a.audio_chunks().is_empty());
        }
        if let Some(m) = work("media/client/os2image/pmdd_6/LEXPS.AVI") {
            let a = Avi::parse(&m).unwrap();
            assert_eq!(&a.video_fourcc(), b"CRAM");
            assert_eq!(a.video_format().bit_count, 8);
        }
    }
}
