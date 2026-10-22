//! CD audio through a CD drive's MMC commands (`BlockDev::packet`): the
//! table of contents, digital audio (READ CD of 2352-byte frames), the
//! position from the Q sub-channel, and play/pause/stop on the drive's own
//! audio output. The CD-ROM IOCtls of categories 0x80 and 0x81
//! (api/cdioctl.rs) are built on these, and MDM's cdaudio device on them.
//!
//! Addresses are logical block addresses here; Red Book (minute, second,
//! frame) addresses are 150 frames (the two-second lead-in) ahead.

use alloc::vec::Vec;

use super::block::BlockDev;
use crate::api::err;

/// Bytes in a raw frame (one sector of digital audio).
pub const FRAME: usize = 2352;
/// Frames per READ CD command: under 64 KB, the most every driver's
/// bounce buffer and an ATAPI byte count limit take.
const FRAMES_PER_READ: usize = 24;

/// A track of the table of contents.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub number: u8,
    /// The descriptor's ADR (high nibble) and CONTROL (low nibble) as on
    /// the disc: CONTROL bit 2 marks a data track.
    pub adr_ctrl: u8,
    pub lba: u32,
}

impl Track {
    pub fn is_data(&self) -> bool {
        self.adr_ctrl & 0x04 != 0
    }
}

pub struct Toc {
    pub first: u8,
    pub last: u8,
    pub tracks: Vec<Track>,
    /// The lead-out: where the last track ends.
    pub leadout: u32,
}

impl Toc {
    pub fn track(&self, n: u8) -> Option<&Track> {
        self.tracks.iter().find(|t| t.number == n)
    }
}

/// The Q sub-channel's current position and the drive's audio status.
#[derive(Clone, Copy, Debug, Default)]
pub struct Position {
    /// MMC audio status: 0x11 playing, 0x12 paused, 0x13 completed,
    /// 0x14 stopped by an error, 0x15 none.
    pub audio: u8,
    pub adr_ctrl: u8,
    pub track: u8,
    pub index: u8,
    pub abs: u32,
    /// In the track (negative in the pause before it).
    pub rel: i32,
}

pub const AUDIO_PLAYING: u8 = 0x11;
pub const AUDIO_PAUSED: u8 = 0x12;

/// LBA to Red Book (minute, second, frame).
pub fn msf(lba: u32) -> (u8, u8, u8) {
    let x = lba + 150;
    ((x / 4500).min(255) as u8, (x / 75 % 60) as u8, (x % 75) as u8)
}

/// Red Book to LBA (None before the lead-in ends or out of range).
pub fn lba(m: u8, s: u8, f: u8) -> Option<u32> {
    if s > 59 || f > 74 {
        return None;
    }
    (m as u32 * 4500 + s as u32 * 75 + f as u32).checked_sub(150)
}

/// A command, tried again once (the first command after a disc change
/// fails with a unit attention).
fn cmd(dev: &dyn BlockDev, cdb: &[u8], buf: &mut [u8]) -> Result<usize, u32> {
    dev.packet(cdb, buf).or_else(|_| dev.packet(cdb, buf))
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// TEST UNIT READY: whether there is a disc and the drive is ready.
pub fn ready(dev: &dyn BlockDev) -> bool {
    cmd(dev, &[0x00, 0, 0, 0, 0, 0], &mut []).is_ok()
}

/// READ TOC/PMA/ATIP, format 0, logical addresses.
pub fn read_toc(dev: &dyn BlockDev) -> Result<Toc, u32> {
    let mut b = alloc::vec![0u8; 4 + 100 * 8];
    let len = b.len() as u16;
    let n = cmd(dev, &[0x43, 0, 0, 0, 0, 0, 0, (len >> 8) as u8, len as u8, 0], &mut b)?;
    if n < 4 {
        return Err(err::NOT_READY);
    }
    let size = (u16::from_be_bytes([b[0], b[1]]) as usize + 2).min(n);
    let mut toc = Toc { first: b[2], last: b[3], tracks: Vec::new(), leadout: 0 };
    for d in b[4..size].chunks_exact(8) {
        let t = Track { number: d[2], adr_ctrl: d[1], lba: be32(&d[4..]) };
        if t.number == 0xaa {
            toc.leadout = t.lba;
        } else if (1..=99).contains(&t.number) {
            toc.tracks.push(t);
        }
    }
    if toc.tracks.is_empty() || toc.leadout == 0 {
        return Err(err::NOT_READY);
    }
    Ok(toc)
}

/// READ CD: `buf.len() / FRAME` raw frames (2352 bytes: audio samples,
/// or a data sector with its sync, header and error correction) from
/// `lba` on.
pub fn read_frames(dev: &dyn BlockDev, lba: u32, buf: &mut [u8]) -> Result<(), u32> {
    if buf.len() % FRAME != 0 {
        return Err(err::INVALID_PARAMETER);
    }
    for (i, chunk) in buf.chunks_mut(FRAMES_PER_READ * FRAME).enumerate() {
        let at = lba + (i * FRAMES_PER_READ) as u32;
        let n = (chunk.len() / FRAME) as u32;
        let a = at.to_be_bytes();
        // Any sector type; sync, header, user data, EDC/ECC: 2352 bytes.
        let cdb = [0xbe, 0, a[0], a[1], a[2], a[3], (n >> 16) as u8, (n >> 8) as u8, n as u8, 0xf8, 0, 0];
        if cmd(dev, &cdb, chunk)? != chunk.len() {
            return Err(err::NOT_READY);
        }
    }
    Ok(())
}

/// READ SUB-CHANNEL: the current position (Q data, format 1).
pub fn position(dev: &dyn BlockDev) -> Result<Position, u32> {
    let mut b = [0u8; 16];
    let n = cmd(dev, &[0x42, 0, 0x40, 0x01, 0, 0, 0, 0, 16, 0], &mut b)?;
    if n < 4 {
        return Err(err::NOT_READY);
    }
    let mut p = Position { audio: b[1], ..Default::default() };
    if n >= 16 {
        p.adr_ctrl = b[5];
        p.track = b[6];
        p.index = b[7];
        p.abs = be32(&b[8..]);
        p.rel = be32(&b[12..]) as i32;
    }
    Ok(p)
}

/// READ SUB-CHANNEL format 2: the disc's media catalog number (13
/// ASCII digits), if it has one.
pub fn catalog_number(dev: &dyn BlockDev) -> Option<[u8; 13]> {
    let mut b = [0u8; 24];
    let n = cmd(dev, &[0x42, 0, 0x40, 0x02, 0, 0, 0, 0, 24, 0], &mut b).ok()?;
    if n < 22 || b[8] & 0x80 == 0 {
        return None;
    }
    let mut out = [0u8; 13];
    out.copy_from_slice(&b[9..22]);
    Some(out)
}

/// PLAY AUDIO MSF from `start` up to (not including) `end`, through the
/// drive's audio output.
pub fn play(dev: &dyn BlockDev, start: u32, end: u32) -> Result<(), u32> {
    let (a, b) = (msf(start), msf(end));
    cmd(dev, &[0x47, 0, 0, a.0, a.1, a.2, b.0, b.1, b.2, 0], &mut []).map(|_| ())
}

/// PAUSE/RESUME.
pub fn pause(dev: &dyn BlockDev, pause: bool) -> Result<(), u32> {
    cmd(dev, &[0x4b, 0, 0, 0, 0, 0, 0, 0, !pause as u8, 0], &mut []).map(|_| ())
}

/// STOP PLAY/SCAN.
pub fn stop(dev: &dyn BlockDev) -> Result<(), u32> {
    cmd(dev, &[0x4e, 0, 0, 0, 0, 0, 0, 0, 0, 0], &mut []).map(|_| ())
}

/// START STOP UNIT: eject the disc (open the tray) or load it.
pub fn eject(dev: &dyn BlockDev, open: bool) -> Result<(), u32> {
    cmd(dev, &[0x1b, 0, 0, 0, if open { 0x02 } else { 0x03 }, 0], &mut []).map(|_| ())
}

/// PREVENT ALLOW MEDIUM REMOVAL.
pub fn lock(dev: &dyn BlockDev, lock: bool) -> Result<(), u32> {
    cmd(dev, &[0x1e, 0, 0, 0, lock as u8, 0], &mut []).map(|_| ())
}

/// SEEK(10).
pub fn seek(dev: &dyn BlockDev, lba: u32) -> Result<(), u32> {
    let a = lba.to_be_bytes();
    cmd(dev, &[0x2b, 0, a[0], a[1], a[2], a[3], 0, 0, 0, 0], &mut []).map(|_| ())
}
