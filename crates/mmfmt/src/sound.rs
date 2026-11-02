//! Other digital audio files, read as PCM for the audio I/O procedures
//! (what MMPM/2's AIFFPROC and SNDBLAST procedures present):
//!
//! - Sun/NeXT `.AU`/`.SND` (`.snd` magic, big-endian; µ-law, A-law,
//!   linear 8/16/24/32-bit);
//! - old headerless `.SND` files are not recognised (no magic);
//! - Creative Voice `.VOC` (sound data, continuation, silence, repeat,
//!   extended and new-format blocks; 8-bit unsigned and 16-bit PCM,
//!   µ-law and A-law);
//! - Apple `AIFF` and `AIFC` (uncompressed `NONE`/`twos`, `sowt`,
//!   `raw `, `ulaw`, `alaw`, `fl32` is not) with the 80-bit sample rate;
//! - Amiga IFF `8SVX` (one-shot and repeat parts, uncompressed).
//!
//! Every format comes out as a [`Sound`]: a [`WaveFormat`] for 8-bit
//! unsigned or 16-bit signed little-endian PCM and the samples, which is
//! what a WAVE file of the same sound would hold. `build_*` write PCM
//! back in each format.

use alloc::vec;
use alloc::vec::Vec;

use crate::wav::{self, WAVE_FORMAT_PCM, WaveFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotThisFormat,
    Truncated,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Au,
    Voc,
    Aiff,
    Svx,
}

/// PCM samples and their format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sound {
    pub format: WaveFormat,
    pub data: Vec<u8>,
}

pub fn pcm_format(channels: u16, rate: u32, bits: u16) -> WaveFormat {
    let block_align = channels * (bits / 8);
    WaveFormat { tag: WAVE_FORMAT_PCM, channels, rate, bits, block_align, avg_bytes: rate * block_align as u32 }
}

fn be16(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

fn le16(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

fn le32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// The format of a file from its first bytes (12 are enough).
pub fn identify(head: &[u8]) -> Option<Kind> {
    if head.starts_with(b".snd") {
        Some(Kind::Au)
    } else if head.starts_with(b"Creative Voice File\x1a") {
        Some(Kind::Voc)
    } else if head.len() >= 12 && &head[0..4] == b"FORM" && (&head[8..12] == b"AIFF" || &head[8..12] == b"AIFC") {
        Some(Kind::Aiff)
    } else if head.len() >= 12 && &head[0..4] == b"FORM" && &head[8..12] == b"8SVX" {
        Some(Kind::Svx)
    } else {
        None
    }
}

pub fn parse(d: &[u8]) -> Result<Sound, Error> {
    match identify(d) {
        Some(Kind::Au) => parse_au(d),
        Some(Kind::Voc) => parse_voc(d),
        Some(Kind::Aiff) => parse_aiff(d),
        Some(Kind::Svx) => parse_svx(d),
        None => Err(Error::NotThisFormat),
    }
}

/// Big-endian signed samples of `bytes` bytes each to 16-bit LE PCM (the
/// top 16 bits).
fn be_to_16(src: &[u8], bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() / bytes * 2);
    for s in src.chunks_exact(bytes) {
        out.push(s[1]);
        out.push(s[0]);
    }
    out
}

fn law_to_16(src: &[u8], f: fn(u8) -> i16) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.extend_from_slice(&f(b).to_le_bytes());
    }
    out
}

/// Signed 8-bit samples to unsigned (WAVE's 8-bit form).
fn signed8_to_unsigned(src: &[u8]) -> Vec<u8> {
    src.iter().map(|&b| b ^ 0x80).collect()
}

// ---- Sun AU -----------------------------------------------------------

pub fn parse_au(d: &[u8]) -> Result<Sound, Error> {
    if !d.starts_with(b".snd") {
        return Err(Error::NotThisFormat);
    }
    let off = be32(d, 4).ok_or(Error::Truncated)? as usize;
    let size = be32(d, 8).ok_or(Error::Truncated)?;
    let enc = be32(d, 12).ok_or(Error::Truncated)?;
    let rate = be32(d, 16).ok_or(Error::Truncated)?;
    let ch = be32(d, 20).ok_or(Error::Truncated)?;
    if off < 24 || ch == 0 || ch > 8 || rate == 0 {
        return Err(Error::Unsupported);
    }
    let body = d.get(off..).ok_or(Error::Truncated)?;
    let body = if size != 0xffff_ffff && (size as usize) < body.len() { &body[..size as usize] } else { body };
    let ch = ch as u16;
    let (bits, data) = match enc {
        1 => (16, law_to_16(body, wav::mulaw)),
        2 => (8, signed8_to_unsigned(body)),
        3 => (16, be_to_16(body, 2)),
        4 => (16, be_to_16(body, 3)),
        5 => (16, be_to_16(body, 4)),
        27 => (16, law_to_16(body, wav::alaw)),
        _ => return Err(Error::Unsupported),
    };
    let format = pcm_format(ch, rate, bits);
    let whole = data.len() / format.block_align as usize * format.block_align as usize;
    let mut data = data;
    data.truncate(whole);
    Ok(Sound { format, data })
}

/// An AU file of 8-bit (signed linear) or 16-bit (linear) PCM.
pub fn build_au(s: &Sound) -> Vec<u8> {
    let (enc, body): (u32, Vec<u8>) = if s.format.bits == 8 {
        (2, signed8_to_unsigned(&s.data))
    } else {
        let mut b = Vec::with_capacity(s.data.len());
        for p in s.data.chunks_exact(2) {
            b.push(p[1]);
            b.push(p[0]);
        }
        (3, b)
    };
    let mut out = Vec::with_capacity(32 + body.len());
    out.extend_from_slice(b".snd");
    for v in [32u32, body.len() as u32, enc, s.format.rate, s.format.channels as u32] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&body);
    out
}

// ---- Creative Voice ----------------------------------------------------

pub fn parse_voc(d: &[u8]) -> Result<Sound, Error> {
    if !d.starts_with(b"Creative Voice File\x1a") {
        return Err(Error::NotThisFormat);
    }
    let mut p = le16(d, 20).ok_or(Error::Truncated)? as usize;
    let mut format: Option<WaveFormat> = None;
    // An extended block's settings for the next sound data block.
    let mut ext: Option<(u32, u16)> = None;
    let mut data = Vec::new();
    let mut repeat: Option<(usize, u16)> = None;
    let mut guard = 0u32;
    while p < d.len() {
        guard += 1;
        if guard > 1_000_000 {
            break;
        }
        let ty = d[p];
        if ty == 0 {
            break;
        }
        let len = (le32(d, p).ok_or(Error::Truncated)? >> 8) as usize;
        let body = p + 4;
        let end = (body + len).min(d.len());
        let b = &d[body..end];
        match ty {
            1 => {
                // Sound data: rate code, codec, samples.
                if b.len() < 2 {
                    break;
                }
                let (rate, ch) = match ext.take() {
                    Some(e) => e,
                    None => (1_000_000 / (256 - b[0] as u32), 1),
                };
                let codec = b[1];
                let s = &b[2..];
                let (f, pcm) = match codec {
                    0 => (pcm_format(ch, rate, 8), s.to_vec()),
                    4 => (pcm_format(ch, rate, 16), s.to_vec()),
                    6 => (pcm_format(ch, rate, 16), law_to_16(s, wav::alaw)),
                    7 => (pcm_format(ch, rate, 16), law_to_16(s, wav::mulaw)),
                    _ => return Err(Error::Unsupported),
                };
                if format.is_none() {
                    format = Some(f);
                }
                data.extend_from_slice(&pcm);
            }
            2 => data.extend_from_slice(b),
            3 => {
                // Silence: length - 1 samples at a rate.
                let n = le16(b, 0).unwrap_or(0) as usize + 1;
                let f = *format.get_or_insert_with(|| pcm_format(1, 1_000_000 / (256 - *b.get(2).unwrap_or(&0) as u32), 8));
                let fill = if f.bits == 8 { 0x80 } else { 0 };
                data.extend(core::iter::repeat_n(fill, n * f.block_align as usize));
            }
            6 => repeat = Some((end, le16(b, 0).unwrap_or(0))),
            7 => {
                if let Some((start, count)) = repeat {
                    if count > 0 && count != 0xffff {
                        repeat = Some((start, count - 1));
                        p = start;
                        continue;
                    }
                    repeat = None;
                }
            }
            8 => {
                // Extended: time constant (16-bit), pack, mode (0 mono, 1 stereo).
                let tc = le16(b, 0).unwrap_or(0) as u32;
                let ch = if b.get(3) == Some(&1) { 2 } else { 1 };
                let rate = 256_000_000 / (65536 - tc.min(65535)) / ch as u32;
                ext = Some((rate, ch));
            }
            9 => {
                // New format: rate, bits, channels, codec, reserved, samples.
                let rate = le32(b, 0).unwrap_or(0);
                let bits = *b.get(4).unwrap_or(&0) as u16;
                let ch = *b.get(5).unwrap_or(&1) as u16;
                let codec = le16(b, 6).unwrap_or(0);
                let s = b.get(12..).unwrap_or(&[]);
                if rate == 0 || ch == 0 {
                    return Err(Error::Unsupported);
                }
                let (f, pcm) = match (codec, bits) {
                    (0, 8) => (pcm_format(ch, rate, 8), s.to_vec()),
                    (4, 16) => (pcm_format(ch, rate, 16), s.to_vec()),
                    (6, _) => (pcm_format(ch, rate, 16), law_to_16(s, wav::alaw)),
                    (7, _) => (pcm_format(ch, rate, 16), law_to_16(s, wav::mulaw)),
                    _ => return Err(Error::Unsupported),
                };
                if format.is_none() {
                    format = Some(f);
                }
                data.extend_from_slice(&pcm);
            }
            _ => {}
        }
        p = end;
    }
    let format = format.ok_or(Error::Truncated)?;
    let whole = data.len() / format.block_align as usize * format.block_align as usize;
    data.truncate(whole);
    Ok(Sound { format, data })
}

/// A VOC file (version 1.20, one new-format block).
pub fn build_voc(s: &Sound) -> Vec<u8> {
    let mut out = Vec::with_capacity(26 + 16 + s.data.len());
    out.extend_from_slice(b"Creative Voice File\x1a");
    out.extend_from_slice(&26u16.to_le_bytes());
    out.extend_from_slice(&0x0114u16.to_le_bytes());
    out.extend_from_slice(&(0x1234u16.wrapping_add(!0x0114u16)).to_le_bytes());
    let mut rest = s.data.as_slice();
    let mut first = true;
    // A block holds at most 16 MB (24-bit length).
    loop {
        let n = rest.len().min(0xff_0000);
        if first {
            let len = (n + 12) as u32;
            out.extend_from_slice(&(9 | len << 8).to_le_bytes());
            out.extend_from_slice(&s.format.rate.to_le_bytes());
            out.push(s.format.bits as u8);
            out.push(s.format.channels as u8);
            out.extend_from_slice(&(if s.format.bits == 16 { 4u16 } else { 0 }).to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            first = false;
        } else {
            out.extend_from_slice(&(2 | (n as u32) << 8).to_le_bytes());
        }
        out.extend_from_slice(&rest[..n]);
        rest = &rest[n..];
        if rest.is_empty() {
            break;
        }
    }
    out.push(0);
    out
}

// ---- AIFF / AIFC / 8SVX --------------------------------------------------

/// IEEE 754 80-bit extended (big-endian) to an integer.
fn ext80(b: &[u8]) -> u32 {
    let exp = (u16::from_be_bytes([b[0], b[1]]) & 0x7fff) as i32;
    let mant = u64::from_be_bytes(b[2..10].try_into().unwrap());
    let shift = exp - 16383 - 63;
    if exp == 0 || shift < -63 {
        return 0;
    }
    if shift >= 0 {
        return u32::MAX;
    }
    (mant >> (-shift) as u32).min(u32::MAX as u64) as u32
}

fn to_ext80(v: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if v == 0 {
        return out;
    }
    let lz = v.leading_zeros();
    let exp = 16383 + 31 - lz;
    out[0..2].copy_from_slice(&(exp as u16).to_be_bytes());
    let mant = (v as u64) << (32 + lz);
    out[2..10].copy_from_slice(&mant.to_be_bytes());
    out
}

/// The chunks of an IFF FORM: (id, body).
fn iff_chunks(d: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut p = 12usize;
    core::iter::from_fn(move || {
        let id = d.get(p..p + 4)?;
        let len = be32(d, p + 4)? as usize;
        let body = &d[(p + 8).min(d.len())..(p + 8).saturating_add(len).min(d.len())];
        p = p + 8 + len + (len & 1);
        Some((id, body))
    })
}

pub fn parse_aiff(d: &[u8]) -> Result<Sound, Error> {
    if identify(d) != Some(Kind::Aiff) {
        return Err(Error::NotThisFormat);
    }
    let aifc = &d[8..12] == b"AIFC";
    let mut comm: Option<(u16, u16, u32, [u8; 4])> = None;
    let mut ssnd: Option<&[u8]> = None;
    for (id, b) in iff_chunks(d) {
        match id {
            b"COMM" if b.len() >= 18 => {
                let ch = be16(b, 0).unwrap();
                let bits = be16(b, 6).unwrap();
                let rate = ext80(&b[8..18]);
                let comp = if aifc && b.len() >= 22 { b[18..22].try_into().unwrap() } else { *b"NONE" };
                comm = Some((ch, bits, rate, comp));
            }
            b"SSND" if b.len() >= 8 => {
                let off = be32(b, 0).unwrap() as usize;
                ssnd = b.get(8 + off..);
            }
            _ => {}
        }
    }
    let (ch, bits, rate, comp) = comm.ok_or(Error::Truncated)?;
    let s = ssnd.unwrap_or(&[]);
    if ch == 0 || ch > 8 || rate == 0 {
        return Err(Error::Unsupported);
    }
    let bytes = bits.div_ceil(8) as usize;
    let (bits, data) = match &comp {
        b"NONE" | b"twos" => match bytes {
            1 => (8, signed8_to_unsigned(s)),
            2..=4 => (16, be_to_16(s, bytes)),
            _ => return Err(Error::Unsupported),
        },
        b"sowt" if bytes == 2 => (16, s.to_vec()),
        b"raw " if bytes == 1 => (8, s.to_vec()),
        b"ulaw" | b"ULAW" => (16, law_to_16(s, wav::mulaw)),
        b"alaw" | b"ALAW" => (16, law_to_16(s, wav::alaw)),
        _ => return Err(Error::Unsupported),
    };
    let format = pcm_format(ch, rate, bits);
    let mut data = data;
    data.truncate(data.len() / format.block_align as usize * format.block_align as usize);
    Ok(Sound { format, data })
}

/// An AIFF file (8-bit or 16-bit PCM).
pub fn build_aiff(s: &Sound) -> Vec<u8> {
    let body: Vec<u8> = if s.format.bits == 8 {
        signed8_to_unsigned(&s.data)
    } else {
        let mut b = Vec::with_capacity(s.data.len());
        for p in s.data.chunks_exact(2) {
            b.push(p[1]);
            b.push(p[0]);
        }
        b
    };
    let frames = body.len() as u32 / s.format.block_align.max(1) as u32;
    let mut out = Vec::with_capacity(54 + body.len());
    out.extend_from_slice(b"FORM");
    let pad = body.len() & 1;
    out.extend_from_slice(&((4 + 26 + 16 + body.len() + pad) as u32).to_be_bytes());
    out.extend_from_slice(b"AIFFCOMM");
    out.extend_from_slice(&18u32.to_be_bytes());
    out.extend_from_slice(&s.format.channels.to_be_bytes());
    out.extend_from_slice(&frames.to_be_bytes());
    out.extend_from_slice(&s.format.bits.to_be_bytes());
    out.extend_from_slice(&to_ext80(s.format.rate));
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&((8 + body.len()) as u32).to_be_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&body);
    if pad != 0 {
        out.push(0);
    }
    out
}

pub fn parse_svx(d: &[u8]) -> Result<Sound, Error> {
    if identify(d) != Some(Kind::Svx) {
        return Err(Error::NotThisFormat);
    }
    let mut rate = 0u32;
    let mut body: Option<&[u8]> = None;
    let mut ch = 1u16;
    for (id, b) in iff_chunks(d) {
        match id {
            b"VHDR" if b.len() >= 16 => {
                rate = be16(b, 12).unwrap() as u32;
                if b[15] != 0 {
                    return Err(Error::Unsupported);
                }
            }
            b"CHAN" if b.len() >= 4 => {
                if be32(b, 0) == Some(6) {
                    ch = 2;
                }
            }
            b"BODY" => body = Some(b),
            _ => {}
        }
    }
    let b = body.ok_or(Error::Truncated)?;
    if rate == 0 {
        return Err(Error::Unsupported);
    }
    let data = if ch == 2 {
        // Stereo 8SVX keeps the left samples, then the right ones.
        let n = b.len() / 2;
        let mut v = vec![0u8; n * 2];
        for i in 0..n {
            v[2 * i] = b[i] ^ 0x80;
            v[2 * i + 1] = b[n + i] ^ 0x80;
        }
        v
    } else {
        signed8_to_unsigned(b)
    };
    Ok(Sound { format: pcm_format(ch, rate, 8), data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone16(n: usize) -> Sound {
        let mut d = Vec::new();
        for i in 0..n {
            let v = ((i as i32 * 977) % 20000 - 10000) as i16;
            d.extend_from_slice(&v.to_le_bytes());
        }
        Sound { format: pcm_format(1, 22050, 16), data: d }
    }

    #[test]
    fn round_trips() {
        let s = tone16(1001);
        assert_eq!(parse(&build_au(&s)).unwrap(), s);
        assert_eq!(parse(&build_aiff(&s)).unwrap(), s);
        assert_eq!(parse(&build_voc(&s)).unwrap(), s);
        let s8 = Sound { format: pcm_format(2, 11025, 8), data: (0..400u32).map(|i| (i * 7) as u8).collect() };
        assert_eq!(parse(&build_au(&s8)).unwrap(), s8);
        assert_eq!(parse(&build_aiff(&s8)).unwrap(), s8);
        assert_eq!(parse(&build_voc(&s8)).unwrap(), s8);
    }

    #[test]
    fn ext80_rates() {
        for r in [8000, 11025, 22050, 44100, 48000, 96000] {
            assert_eq!(ext80(&to_ext80(r)), r);
        }
    }

    #[test]
    fn old_voc_block() {
        // Version 1.10 file with one type-1 block: 11111 Hz (tc 166), 8-bit.
        let mut f = b"Creative Voice File\x1a".to_vec();
        f.extend_from_slice(&[26, 0, 0x0a, 0x01, 0x29, 0x11]);
        f.extend_from_slice(&[1, 6, 0, 0, 166, 0, 1, 2, 3, 4]);
        f.push(0);
        let s = parse(&f).unwrap();
        assert_eq!(s.format, pcm_format(1, 11111, 8));
        assert_eq!(s.data, [1, 2, 3, 4]);
    }

    #[test]
    fn au_mulaw() {
        let mut f = b".snd".to_vec();
        for v in [24u32, 2, 1, 8000, 1] {
            f.extend_from_slice(&v.to_be_bytes());
        }
        f.extend_from_slice(&[0xff, 0x00]);
        let s = parse(&f).unwrap();
        assert_eq!(s.format, pcm_format(1, 8000, 16));
        assert_eq!(s.data.len(), 4);
        assert_eq!(i16::from_le_bytes([s.data[0], s.data[1]]), wav::mulaw(0xff));
    }

    #[test]
    fn bad_input() {
        for n in 0..60 {
            let mut f = build_voc(&tone16(10));
            f.truncate(n);
            let _ = parse(&f);
            let mut f = build_aiff(&tone16(10));
            f.truncate(n);
            let _ = parse(&f);
            let mut f = build_au(&tone16(10));
            f.truncate(n);
            let _ = parse(&f);
        }
    }
}
