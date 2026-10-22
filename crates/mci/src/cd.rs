//! CD audio times: the table of contents of a disc and the conversions
//! between disc addresses and the MCI time formats.
//!
//! Positions are Red Book frame addresses (75 a second, counted from the
//! start of the lead-in, so the first track usually starts at 150 =
//! 00:02:00); a logical block address is 150 less. In MILLISECONDS,
//! MMTIME (1/3000 s) and MSF a position is the address itself; in TMSF it
//! is the track and the time since that track started. Lengths are
//! durations: TMSF shows them as MSF.

use alloc::string::String;
use alloc::vec::Vec;

pub const FRAMES_PER_SECOND: u32 = 75;
/// Frames before LBA 0 (the two-second lead-in).
pub const LEAD_IN: u32 = 150;

pub const MCI_FORMAT_MILLISECONDS: u32 = 1;
pub const MCI_FORMAT_MMTIME: u32 = 2;
pub const MCI_FORMAT_MSF: u32 = 5;
pub const MCI_FORMAT_TMSF: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    pub number: u8,
    /// Red Book frame address of the track's start.
    pub start: u32,
    pub data: bool,
    /// CONTROL bits of the track: 1 pre-emphasis, 2 digital copy
    /// permitted, 8 four channels.
    pub control: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Toc {
    pub tracks: Vec<Track>,
    /// Red Book address of the lead-out (the end of the last track).
    pub leadout: u32,
}

impl Toc {
    pub fn first(&self) -> u8 {
        self.tracks.first().map_or(0, |t| t.number)
    }

    pub fn last(&self) -> u8 {
        self.tracks.last().map_or(0, |t| t.number)
    }

    pub fn track(&self, n: u8) -> Option<&Track> {
        self.tracks.iter().find(|t| t.number == n)
    }

    /// Where track `n` ends: the next track's start or the lead-out.
    pub fn track_end(&self, n: u8) -> Option<u32> {
        let i = self.tracks.iter().position(|t| t.number == n)?;
        Some(self.tracks.get(i + 1).map_or(self.leadout, |t| t.start))
    }

    /// The track an address is in (the first one before the first track).
    pub fn track_at(&self, a: u32) -> Option<&Track> {
        self.tracks.iter().rev().find(|t| t.start <= a).or(self.tracks.first())
    }

    /// Where the disc's audio starts: the first audio track.
    pub fn start(&self) -> u32 {
        self.tracks.iter().find(|t| !t.data).or(self.tracks.first()).map_or(LEAD_IN, |t| t.start)
    }

    pub fn has_audio(&self) -> bool {
        self.tracks.iter().any(|t| !t.data)
    }
}

/// Pack a time: the first value in the low byte ("t:m:s:f" in TMSF,
/// "m:s:f" in MSF).
pub fn pack(v: &[u32]) -> u32 {
    v.iter().enumerate().fold(0, |acc, (i, &x)| acc | (x & 0xff) << (8 * i))
}

fn msf_parts(frames: u32) -> (u32, u32, u32) {
    (frames / (60 * FRAMES_PER_SECOND), frames / FRAMES_PER_SECOND % 60, frames % FRAMES_PER_SECOND)
}

/// Frames as a packed MSF value.
pub fn to_msf(frames: u32) -> u32 {
    let (m, s, f) = msf_parts(frames);
    pack(&[m.min(255), s, f])
}

/// A packed MSF value as frames.
pub fn from_msf(v: u32) -> u32 {
    (v & 0xff) * 60 * FRAMES_PER_SECOND + (v >> 8 & 0xff) * FRAMES_PER_SECOND + (v >> 16 & 0xff)
}

/// A position (Red Book address) in a time format.
pub fn position_to(toc: &Toc, a: u32, fmt: u32) -> u32 {
    match fmt {
        MCI_FORMAT_MILLISECONDS => (a as u64 * 1000 / FRAMES_PER_SECOND as u64) as u32,
        MCI_FORMAT_MSF => to_msf(a),
        MCI_FORMAT_TMSF => match toc.track_at(a) {
            Some(t) => to_msf(a.saturating_sub(t.start)) << 8 | t.number as u32,
            None => 0,
        },
        _ => a * (3000 / FRAMES_PER_SECOND),
    }
}

/// A position in a time format as a Red Book address; TMSF needs the
/// track to exist (MCIERR_OUTOFRANGE otherwise, as a None).
pub fn position_from(toc: &Toc, v: u32, fmt: u32) -> Option<u32> {
    Some(match fmt {
        MCI_FORMAT_MILLISECONDS => (v as u64 * FRAMES_PER_SECOND as u64 / 1000) as u32,
        MCI_FORMAT_MSF => from_msf(v),
        MCI_FORMAT_TMSF => toc.track((v & 0xff) as u8)?.start + from_msf(v >> 8),
        _ => v / (3000 / FRAMES_PER_SECOND),
    })
}

/// A length in frames in a time format (TMSF as MSF).
pub fn length_to(frames: u32, fmt: u32) -> u32 {
    match fmt {
        MCI_FORMAT_MILLISECONDS => (frames as u64 * 1000 / FRAMES_PER_SECOND as u64) as u32,
        MCI_FORMAT_MSF | MCI_FORMAT_TMSF => to_msf(frames),
        _ => frames * (3000 / FRAMES_PER_SECOND),
    }
}

/// How the string interface shows a time: "tt:mm:ss:ff" (TMSF),
/// "mm:ss:ff" (MSF, and lengths in TMSF), else the number.
pub fn time_text(v: u32, fmt: u32, length: bool) -> String {
    let b = |i: u32| v >> (8 * i) & 0xff;
    match fmt {
        MCI_FORMAT_TMSF if !length => alloc::format!("{:02}:{:02}:{:02}:{:02}", b(0), b(1), b(2), b(3)),
        MCI_FORMAT_MSF | MCI_FORMAT_TMSF => alloc::format!("{:02}:{:02}:{:02}", b(0), b(1), b(2)),
        _ => alloc::format!("{v}"),
    }
}

/// A time as a string command writes it: a number, or values separated
/// by colons ("2", "2:30", "01:00:30:00") packed low byte first.
pub fn parse_time(s: &str) -> Option<u32> {
    if !s.contains(':') {
        return crate::parse_number(s);
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut v = Vec::new();
    for p in parts {
        let n: u32 = p.trim().parse().ok()?;
        if n > 255 {
            return None;
        }
        v.push(n);
    }
    Some(pack(&v))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A data track, then two audio tracks (as a mixed-mode disc).
    fn toc() -> Toc {
        Toc {
            tracks: alloc::vec![
                Track { number: 1, start: 150, data: true, control: 4 },
                Track { number: 2, start: 150 + 75 * 60, data: false, control: 0 },
                Track { number: 3, start: 150 + 75 * 200, data: false, control: 2 },
            ],
            leadout: 150 + 75 * 300,
        }
    }

    #[test]
    fn tracks() {
        let t = toc();
        assert_eq!((t.first(), t.last()), (1, 3));
        assert_eq!(t.start(), 150 + 75 * 60);
        assert_eq!(t.track_end(2), Some(150 + 75 * 200));
        assert_eq!(t.track_end(3), Some(t.leadout));
        assert_eq!(t.track_at(150 + 75 * 60 + 10).unwrap().number, 2);
        assert_eq!(t.track_at(0).unwrap().number, 1);
    }

    #[test]
    fn formats() {
        let t = toc();
        // 00:02:00 is the first track's start.
        assert_eq!(position_to(&t, 150, MCI_FORMAT_MSF), pack(&[0, 2, 0]));
        assert_eq!(position_to(&t, 150, MCI_FORMAT_MILLISECONDS), 2000);
        assert_eq!(position_to(&t, 150, MCI_FORMAT_MMTIME), 6000);
        // Track 2 plus 1:30:10.
        let a = 150 + 75 * 60 + 90 * 75 + 10;
        let tmsf = position_to(&t, a, MCI_FORMAT_TMSF);
        assert_eq!(tmsf, pack(&[2, 1, 30, 10]));
        assert_eq!(time_text(tmsf, MCI_FORMAT_TMSF, false), "02:01:30:10");
        for f in [MCI_FORMAT_MSF, MCI_FORMAT_TMSF, MCI_FORMAT_MMTIME] {
            assert_eq!(position_from(&t, position_to(&t, a, f), f), Some(a));
        }
        assert_eq!(position_from(&t, 3, MCI_FORMAT_TMSF), Some(150 + 75 * 200));
        assert_eq!(position_from(&t, 9, MCI_FORMAT_TMSF), None);
        assert_eq!(length_to(75 * 100, MCI_FORMAT_TMSF), pack(&[1, 40, 0]));
        assert_eq!(time_text(length_to(75 * 100, MCI_FORMAT_TMSF), MCI_FORMAT_TMSF, true), "01:40:00");
    }

    #[test]
    fn parsing() {
        assert_eq!(parse_time("2"), Some(2));
        assert_eq!(parse_time("2:30"), Some(pack(&[2, 30])));
        assert_eq!(parse_time("01:00:30:00"), Some(pack(&[1, 0, 30, 0])));
        assert_eq!(parse_time("1:2:3:4:5"), None);
        assert_eq!(parse_time("1:x"), None);
    }
}
