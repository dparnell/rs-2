//! MIDI for rs2's MMPM/2: Standard MIDI files (`smf`), a small General
//! MIDI software synthesizer (`synth`) and a player that renders a song
//! to PCM from any position (`Player`). `no_std` with `alloc`; MDM.DLL's
//! sequencer device streams the player's output to the sound driver.

#![no_std]

extern crate alloc;

pub mod smf;
pub mod synth;

pub use smf::{parse, Error, Song};
pub use synth::Synth;

/// A song being played: the synthesizer fed with the song's events as
/// rendering reaches their time.
pub struct Player {
    pub song: Song,
    pub synth: Synth,
    /// The next event to send.
    next: usize,
    /// Rendered position in microseconds (exact: kept in frames).
    frames: u64,
}

impl Player {
    pub fn new(song: Song, rate: u32) -> Player {
        Player { song, synth: Synth::new(rate), next: 0, frames: 0 }
    }

    pub fn position_us(&self) -> u64 {
        self.frames * 1_000_000 / self.synth.rate() as u64
    }

    pub fn length_us(&self) -> u64 {
        self.song.length_us
    }

    pub fn finished(&self) -> bool {
        self.next >= self.song.events.len() && self.synth.active() == 0 && self.position_us() >= self.song.length_us
    }

    /// Go to `us` from the start: notes stop; controllers and programs
    /// set before that point are sent again so the music sounds right
    /// from there.
    pub fn seek(&mut self, us: u64) {
        self.synth.reset();
        let i = self.song.event_at(us);
        for e in &self.song.events[..i] {
            if matches!(e.msg.kind(), 0xb0 | 0xc0 | 0xe0) {
                self.synth.message(e.msg);
            }
        }
        self.next = i;
        self.frames = us * self.synth.rate() as u64 / 1_000_000;
    }

    /// Render the next `out.len() / 2` stereo frames (16-bit, overwriting
    /// `out`).
    pub fn render(&mut self, out: &mut [i16]) {
        out.fill(0);
        let rate = self.synth.rate() as u64;
        let mut done = 0;
        let total = out.len() / 2;
        while done < total {
            // Events due at this frame.
            let now_us = self.frames * 1_000_000 / rate;
            while let Some(e) = self.song.events.get(self.next) {
                if e.us > now_us {
                    break;
                }
                self.synth.message(e.msg);
                self.next += 1;
            }
            // Up to the next event (in blocks of at most 64 frames for
            // timing accuracy).
            let next_frame = self.song.events.get(self.next).map_or(u64::MAX, |e| (e.us * rate).div_ceil(1_000_000));
            let n = ((next_frame.saturating_sub(self.frames)).max(1) as usize).min(64).min(total - done);
            self.synth.render(&mut out[done * 2..(done + n) * 2]);
            self.frames += n as u64;
            done += n;
        }
    }
}

#[cfg(test)]
mod tests;
