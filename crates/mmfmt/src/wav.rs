//! RIFF WAVE audio files.
//!
//! [`parse`] finds the format (`fmt `) chunk and the sample (`data`)
//! chunk. Plain PCM (8-bit unsigned or 16-bit signed, mono or stereo) is
//! what MMPM/2's audio devices play as it is; [`decode_pcm16`] also turns
//! µ-law, A-law and IMA ADPCM data into 16-bit PCM.

use alloc::vec::Vec;

use crate::riff::{self, DATA, FMT, WAVE};
use crate::{le16, le32};

/// `wFormatTag` values.
pub const WAVE_FORMAT_PCM: u16 = 1;
pub const WAVE_FORMAT_ALAW: u16 = 6;
pub const WAVE_FORMAT_MULAW: u16 = 7;
pub const WAVE_FORMAT_IMA_ADPCM: u16 = 0x11;

/// The format chunk (`WAVEFORMAT` / MMPM/2's `WAVE_HEADER`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveFormat {
    pub tag: u16,
    pub channels: u16,
    /// Samples per second.
    pub rate: u32,
    /// Bits per sample (4 for IMA ADPCM).
    pub bits: u16,
    /// Bytes per sample frame (all channels); the ADPCM block size.
    pub block_align: u16,
    pub avg_bytes: u32,
}

impl WaveFormat {
    /// PCM that devices play unconverted: 8 or 16 bits, 1 or 2 channels.
    pub fn is_plain_pcm(&self) -> bool {
        self.tag == WAVE_FORMAT_PCM && matches!(self.bits, 8 | 16) && matches!(self.channels, 1 | 2)
    }

    /// Whether [`decode_pcm16`] can decode this format.
    pub fn is_decodable(&self) -> bool {
        matches!(self.channels, 1 | 2)
            && match self.tag {
                WAVE_FORMAT_PCM => matches!(self.bits, 8 | 16),
                WAVE_FORMAT_ALAW | WAVE_FORMAT_MULAW => self.bits == 8,
                WAVE_FORMAT_IMA_ADPCM => self.bits == 4 && self.block_align as usize > 4 * self.channels as usize,
                _ => false,
            }
    }

    /// Sample frames in `bytes` of data.
    pub fn frames(&self, bytes: u32) -> u32 {
        match self.tag {
            WAVE_FORMAT_IMA_ADPCM if self.block_align != 0 => {
                let ba = self.block_align as u32;
                let full = bytes / ba * self.samples_per_block();
                let rest = bytes % ba;
                let hdr = 4 * self.channels as u32;
                full + if rest > hdr { (rest - hdr) * 2 / self.channels as u32 + 1 } else { 0 }
            }
            _ if self.block_align != 0 => bytes / self.block_align as u32,
            _ => 0,
        }
    }

    /// Play time of `bytes` of data in milliseconds.
    pub fn duration_ms(&self, bytes: u32) -> u32 {
        if self.rate == 0 {
            return 0;
        }
        (self.frames(bytes) as u64 * 1000 / self.rate as u64) as u32
    }

    /// Samples per channel in an IMA ADPCM block.
    pub fn samples_per_block(&self) -> u32 {
        let hdr = 4 * self.channels as u32;
        (self.block_align as u32).saturating_sub(hdr) * 2 / self.channels.max(1) as u32 + 1
    }

    /// The format 16-bit PCM decoded by [`decode_pcm16`] has.
    pub fn pcm16(&self) -> WaveFormat {
        let block_align = 2 * self.channels;
        WaveFormat { tag: WAVE_FORMAT_PCM, channels: self.channels, rate: self.rate, bits: 16, block_align, avg_bytes: self.rate * block_align as u32 }
    }

    /// Parse a format chunk's data.
    pub fn parse(d: &[u8]) -> Option<WaveFormat> {
        Some(WaveFormat {
            tag: le16(d, 0)?,
            channels: le16(d, 2)?,
            rate: le32(d, 4)?,
            avg_bytes: le32(d, 8)?,
            block_align: le16(d, 12)?,
            bits: le16(d, 14).unwrap_or(8),
        })
    }

    /// The 16-byte format chunk data.
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0; 16];
        b[0..2].copy_from_slice(&self.tag.to_le_bytes());
        b[2..4].copy_from_slice(&self.channels.to_le_bytes());
        b[4..8].copy_from_slice(&self.rate.to_le_bytes());
        b[8..12].copy_from_slice(&self.avg_bytes.to_le_bytes());
        b[12..14].copy_from_slice(&self.block_align.to_le_bytes());
        b[14..16].copy_from_slice(&self.bits.to_le_bytes());
        b
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a RIFF WAVE file.
    NotWave,
    /// No (or a short) format chunk.
    NoFormat,
    /// No data chunk.
    NoData,
}

/// A parsed WAVE file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wave {
    pub format: WaveFormat,
    /// Offset of the sample data in the file.
    pub data_offset: usize,
    /// Length of the sample data the data chunk declares.
    pub data_len: usize,
}

impl Wave {
    /// The sample data in `file`, cut short when the file is.
    pub fn data<'a>(&self, file: &'a [u8]) -> &'a [u8] {
        let end = self.data_offset.saturating_add(self.data_len).min(file.len());
        file.get(self.data_offset..end).unwrap_or(&[])
    }
}

/// Parse a WAVE file. `file` may be just the start of it, up to the data
/// chunk's header: the data range is what the chunk declares.
pub fn parse(file: &[u8]) -> Result<Wave, Error> {
    let (form, riff) = riff::form(file).ok_or(Error::NotWave)?;
    if form != WAVE {
        return Err(Error::NotWave);
    }
    let mut format = None;
    for c in riff.children() {
        match c.header.id {
            FMT => format = Some(WaveFormat::parse(c.data).ok_or(Error::NoFormat)?),
            DATA => {
                let format = format.ok_or(Error::NoFormat)?;
                return Ok(Wave { format, data_offset: c.offset, data_len: c.header.size as usize });
            }
            _ => {}
        }
    }
    Err(if format.is_some() { Error::NoData } else { Error::NoFormat })
}

/// Decode sample data to interleaved 16-bit PCM; None when the format is
/// not one [`WaveFormat::is_decodable`] accepts.
pub fn decode_pcm16(f: &WaveFormat, d: &[u8]) -> Option<Vec<i16>> {
    if !f.is_decodable() {
        return None;
    }
    Some(match (f.tag, f.bits) {
        (WAVE_FORMAT_PCM, 8) => d.iter().map(|&b| ((b as i16) - 128) << 8).collect(),
        (WAVE_FORMAT_PCM, _) => d.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]])).collect(),
        (WAVE_FORMAT_ALAW, _) => d.iter().map(|&b| alaw(b)).collect(),
        (WAVE_FORMAT_MULAW, _) => d.iter().map(|&b| mulaw(b)).collect(),
        _ => ima_adpcm(f, d),
    })
}

/// G.711 µ-law to linear.
pub fn mulaw(b: u8) -> i16 {
    let b = !b;
    let t = ((((b & 0x0f) as i32) << 3) + 0x84) << ((b >> 4) & 7);
    (if b & 0x80 != 0 { 0x84 - t } else { t - 0x84 }) as i16
}

/// G.711 A-law to linear.
pub fn alaw(b: u8) -> i16 {
    let b = b ^ 0x55;
    let seg = (b >> 4) & 7;
    let mut t = ((b & 0x0f) as i32) << 4;
    t = match seg {
        0 => t + 8,
        1 => t + 0x108,
        _ => (t + 0x108) << (seg - 1),
    };
    (if b & 0x80 != 0 { t } else { -t }) as i16
}

const IMA_INDEX: [i8; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];
const IMA_STEP: [u16; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157,
    173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707,
    1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635,
    13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

struct Ima {
    pred: i32,
    index: i32,
}

impl Ima {
    fn step(&mut self, n: u8) -> i16 {
        let step = IMA_STEP[self.index as usize] as i32;
        let mut diff = step >> 3;
        if n & 1 != 0 {
            diff += step >> 2;
        }
        if n & 2 != 0 {
            diff += step >> 1;
        }
        if n & 4 != 0 {
            diff += step;
        }
        self.pred = if n & 8 != 0 { self.pred - diff } else { self.pred + diff }.clamp(-32768, 32767);
        self.index = (self.index + IMA_INDEX[n as usize & 15] as i32).clamp(0, 88);
        self.pred as i16
    }
}

/// IMA (DVI) ADPCM, as in WAVE files: blocks of `block_align` bytes, each
/// starting with a 4-byte header per channel (first sample, step index),
/// then 4-byte groups of eight 4-bit samples per channel in turn.
fn ima_adpcm(f: &WaveFormat, d: &[u8]) -> Vec<i16> {
    let ch = f.channels as usize;
    let mut out = Vec::with_capacity(f.frames(d.len() as u32) as usize * ch);
    for block in d.chunks(f.block_align as usize) {
        if block.len() <= 4 * ch {
            break;
        }
        let mut st: Vec<Ima> = (0..ch)
            .map(|c| Ima { pred: i16::from_le_bytes([block[4 * c], block[4 * c + 1]]) as i32, index: (block[4 * c + 2] as i32).clamp(0, 88) })
            .collect();
        out.extend(st.iter().map(|s| s.pred as i16));
        let body = &block[4 * ch..];
        // Each round has 4 bytes (8 samples) per channel.
        for round in body.chunks(4 * ch) {
            let n = round.len() / ch * 2;
            let base = out.len();
            out.resize(base + n * ch, 0);
            for (c, s) in st.iter_mut().enumerate() {
                for (i, &b) in round[4 * c..].iter().take(n / 2).enumerate() {
                    out[base + (2 * i) * ch + c] = s.step(b & 15);
                    out[base + (2 * i + 1) * ch + c] = s.step(b >> 4);
                }
            }
        }
    }
    out
}

/// A WAVE file holding 16-bit PCM or 8-bit PCM `data` in format `f`.
pub fn build(f: &WaveFormat, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(44 + data.len() + 1);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + riff::padded(data.len() as u32)).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&f.to_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data.len() as u32).to_le_bytes());
    v.extend_from_slice(data);
    if data.len() & 1 != 0 {
        v.push(0);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm8() -> WaveFormat {
        WaveFormat { tag: 1, channels: 1, rate: 11025, bits: 8, block_align: 1, avg_bytes: 11025 }
    }

    #[test]
    fn round_trip() {
        let data: Vec<u8> = (0..101u8).collect();
        let file = build(&pcm8(), &data);
        let w = parse(&file).unwrap();
        assert_eq!(w.format, pcm8());
        assert_eq!(w.data_offset, 44);
        assert_eq!(w.data(&file), &data[..]);
        assert!(w.format.is_plain_pcm());
        assert_eq!(w.format.duration_ms(11025), 1000);
        let pcm = decode_pcm16(&w.format, w.data(&file)).unwrap();
        assert_eq!(pcm[0], -128 << 8);
        assert_eq!(pcm[100], (100 - 128) << 8);
    }

    #[test]
    fn skips_other_chunks_and_rejects_others() {
        let mut file = build(&pcm8(), &[1, 2, 3]);
        // Insert a LIST chunk before fmt.
        let list = b"LIST\x06\x00\x00\x00INFOab";
        file.splice(12..12, list.iter().copied());
        let len = file.len() as u32 - 8;
        file[4..8].copy_from_slice(&len.to_le_bytes());
        let w = parse(&file).unwrap();
        assert_eq!(w.data(&file), &[1, 2, 3]);
        assert_eq!(parse(b"RIFF\x04\x00\x00\x00AVI "), Err(Error::NotWave));
        assert_eq!(parse(b"RIFF\x04\x00\x00\x00WAVE"), Err(Error::NoFormat));
    }

    #[test]
    fn g711() {
        assert_eq!(mulaw(0xff), 0);
        assert_eq!(mulaw(0x00), -32124);
        assert_eq!(mulaw(0x80), 32124);
        assert_eq!(alaw(0xd5), 8);
        assert_eq!(alaw(0x55), -8);
        assert_eq!(alaw(0xaa), 32256);
    }

    #[test]
    fn ima_block() {
        let f = WaveFormat { tag: 0x11, channels: 1, rate: 8000, bits: 4, block_align: 8, avg_bytes: 4000 };
        assert_eq!(f.samples_per_block(), 9);
        // pred 100, index 0, then nibbles 0x7 (up) ... 0xf (down).
        let block = [100, 0, 0, 0, 0x77, 0xff, 0x00, 0x08];
        let pcm = decode_pcm16(&f, &block).unwrap();
        assert_eq!(pcm.len(), 9);
        assert_eq!(pcm[0], 100);
        assert_eq!(pcm[1], 100 + 7 + 3 + 1); // step 7: 0+3+1+7 = 11 (7>>3=0)
        assert!(pcm[3] < pcm[2]);
        assert_eq!(f.frames(8), 9);
    }
}
