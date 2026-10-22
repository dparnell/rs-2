//! Standard MIDI files (format 0, 1 and 2): the tracks merged into one
//! list of timed events, with the tempo map to turn ticks into time.

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not a MIDI file (no MThd).
    NotMidi,
    /// The file ends inside a chunk or an event.
    Truncated,
    /// SMPTE time division (not supported).
    Smpte,
}

/// A channel message (status byte with its channel, and its data bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message {
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

impl Message {
    pub fn channel(&self) -> u8 {
        self.status & 0x0f
    }
    pub fn kind(&self) -> u8 {
        self.status & 0xf0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// Ticks from the start of the song.
    pub tick: u64,
    /// Microseconds from the start of the song (from the tempo map).
    pub us: u64,
    pub msg: Message,
}

pub struct Song {
    pub format: u16,
    /// Ticks per quarter note.
    pub division: u16,
    /// Channel events in time order.
    pub events: Vec<Event>,
    /// (tick, microseconds per quarter note).
    pub tempos: Vec<(u64, u32)>,
    /// Length in ticks and microseconds (the last event, or the end of
    /// the longest track).
    pub length_ticks: u64,
    pub length_us: u64,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, Error> {
        let b = *self.d.get(self.p).ok_or(Error::Truncated)?;
        self.p += 1;
        Ok(b)
    }
    fn var(&mut self) -> Result<u32, Error> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            v = v << 7 | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Ok(v)
    }
    fn skip(&mut self, n: usize) -> Result<(), Error> {
        if self.p + n > self.d.len() {
            return Err(Error::Truncated);
        }
        self.p += n;
        Ok(())
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Data bytes a channel message of this status has.
fn data_len(status: u8) -> usize {
    match status & 0xf0 {
        0xc0 | 0xd0 => 1,
        _ => 2,
    }
}

pub fn parse(d: &[u8]) -> Result<Song, Error> {
    // A RIFF RMID wraps an SMF in its "data" chunk.
    let d = if d.len() > 20 && &d[0..4] == b"RIFF" && &d[8..12] == b"RMID" {
        let mut p = 12;
        let mut inner = None;
        while p + 8 <= d.len() {
            let n = u32::from_le_bytes([d[p + 4], d[p + 5], d[p + 6], d[p + 7]]) as usize;
            if &d[p..p + 4] == b"data" {
                inner = d.get(p + 8..(p + 8 + n).min(d.len()));
                break;
            }
            p += 8 + n + (n & 1);
        }
        inner.ok_or(Error::NotMidi)?
    } else {
        d
    };
    if d.len() < 14 || &d[0..4] != b"MThd" {
        return Err(Error::NotMidi);
    }
    let hlen = be32(&d[4..8]) as usize;
    if d.len() < 8 + hlen || hlen < 6 {
        return Err(Error::Truncated);
    }
    let format = u16::from_be_bytes([d[8], d[9]]);
    let ntrks = u16::from_be_bytes([d[10], d[11]]);
    let division = u16::from_be_bytes([d[12], d[13]]);
    if division & 0x8000 != 0 {
        return Err(Error::Smpte);
    }
    let division = division.max(1);
    let mut p = 8 + hlen;
    let mut raw: Vec<(u64, Message)> = Vec::new();
    let mut tempos: Vec<(u64, u32)> = Vec::new();
    let mut end_tick = 0u64;
    // Format 2: the tracks are independent patterns, played one after
    // the other.
    let mut base = 0u64;
    for _ in 0..ntrks {
        if p + 8 > d.len() {
            break;
        }
        let len = be32(&d[p + 4..p + 8]) as usize;
        let body = &d[p + 8..(p + 8 + len).min(d.len())];
        let is_track = &d[p..p + 4] == b"MTrk";
        p += 8 + len;
        if !is_track {
            continue;
        }
        let mut r = Reader { d: body, p: 0 };
        let mut tick = base;
        let mut running = 0u8;
        while r.p < body.len() {
            let Ok(delta) = r.var() else { break };
            tick += delta as u64;
            let Ok(mut b) = r.u8() else { break };
            match b {
                0xff => {
                    let Ok(kind) = r.u8() else { break };
                    let Ok(n) = r.var() else { break };
                    let n = n as usize;
                    if kind == 0x51 && n >= 3 && r.p + 3 <= body.len() {
                        let t = (body[r.p] as u32) << 16 | (body[r.p + 1] as u32) << 8 | body[r.p + 2] as u32;
                        tempos.push((tick, t.max(1)));
                    }
                    if r.skip(n).is_err() || kind == 0x2f {
                        break;
                    }
                }
                0xf0 | 0xf7 => {
                    let Ok(n) = r.var() else { break };
                    if r.skip(n as usize).is_err() {
                        break;
                    }
                }
                _ => {
                    let status = if b & 0x80 != 0 {
                        running = b;
                        let Ok(x) = r.u8() else { break };
                        b = x;
                        running
                    } else {
                        running
                    };
                    if status < 0x80 {
                        break;
                    }
                    let data2 = if data_len(status) == 2 {
                        let Ok(x) = r.u8() else { break };
                        x
                    } else {
                        0
                    };
                    raw.push((tick, Message { status, data1: b & 0x7f, data2: data2 & 0x7f }));
                }
            }
        }
        end_tick = end_tick.max(tick);
        if format == 2 {
            base = tick;
        }
    }
    raw.sort_by_key(|e| e.0);
    tempos.sort_by_key(|t| t.0);
    let mut song = Song { format, division, events: Vec::with_capacity(raw.len()), tempos, length_ticks: end_tick, length_us: 0 };
    for (tick, msg) in raw {
        let us = song.tick_to_us(tick);
        song.events.push(Event { tick, us, msg });
    }
    song.length_us = song.tick_to_us(end_tick);
    Ok(song)
}

impl Song {
    /// Microseconds from the start to `tick`, through the tempo map
    /// (120 beats per minute until the first tempo event).
    pub fn tick_to_us(&self, tick: u64) -> u64 {
        let div = self.division as u64;
        let mut us = 0u64;
        let mut at = 0u64;
        let mut tempo = 500_000u64;
        for &(t, v) in &self.tempos {
            if t >= tick {
                break;
            }
            us += (t - at) * tempo / div;
            at = t;
            tempo = v as u64;
        }
        us + (tick - at) * tempo / div
    }

    /// The tick at `us` microseconds (the inverse of `tick_to_us`).
    pub fn us_to_tick(&self, target: u64) -> u64 {
        let div = self.division as u64;
        let mut us = 0u64;
        let mut at = 0u64;
        let mut tempo = 500_000u64;
        for &(t, v) in &self.tempos {
            let span = (t - at) * tempo / div;
            if us + span > target {
                break;
            }
            us += span;
            at = t;
            tempo = v as u64;
        }
        at + (target - us) * div / tempo.max(1)
    }

    /// The index of the first event at or after `us`.
    pub fn event_at(&self, us: u64) -> usize {
        self.events.partition_point(|e| e.us < us)
    }
}
