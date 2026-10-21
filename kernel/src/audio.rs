//! Sound output: the software mixer between the RS2API audio calls
//! (`api::audio`) and the sound hardware ([`Output`]: `drivers::hda`,
//! `drivers::ac97` or `drivers::es137x`).
//!
//! Programs open streams with their own format (8000-48000 Hz and
//! beyond, 8-bit unsigned or 16-bit signed, mono or stereo) and queue raw
//! PCM. The timer tick keeps the hardware ring [`LEAD_MS`] ahead of
//! the hardware's play position: each stream still playing is converted
//! to 16-bit stereo, resampled to the hardware's rate (48 kHz, 44.1 kHz
//! on the ES1370) by linear interpolation, scaled
//! by its volume and summed; the sum is scaled by the master volume and
//! clipped. While nothing is open the ring is filled with silence, and
//! the hardware stops after [`IDLE_STOP_MS`].
//!
//! A stream's play position comes from marks the mixer leaves as it
//! mixes (ring frame, input bytes consumed so far): a mark counts once
//! the hardware has passed its frame.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::drivers::{ac97, es137x, hda};
use crate::sync::SpinLock;
use crate::task::WaitQueue;

/// Frames in every driver's output ring (stereo, 16-bit: 4 bytes each).
pub const RING_FRAMES: usize = 16384;
/// How far ahead of the hardware the ring is filled (100 ms).
const LEAD_MS: u64 = 100;
/// Refill when at least this much of the ring is free (5 ms).
const MIN_FILL_MS: u64 = 5;
/// Frames mixed per step (one mark each).
const CHUNK: usize = 256;
/// A running output whose position stands still this long is taken to
/// be stuck: the mixer paces itself by the clock instead (silently).
const STALL_MS: u64 = 1000;
/// Stop the hardware after this long with no stream open.
const IDLE_STOP_MS: u64 = 2000;
/// Streams in the system.
const MAX_STREAMS: usize = 64;

pub struct Stream {
    pub id: u32,
    pub pid: u32,
    pub rate: u32,
    channels: u32,
    bits: u32,
    queue: VecDeque<u8>,
    pub volume: u32,
    pub paused: bool,
    /// Input bytes queued in all, taken by the mixer, and played.
    pub written: u64,
    consumed: u64,
    pub played: u64,
    /// (ring frame, consumed) pairs not yet reached by the hardware.
    marks: VecDeque<(u64, u64)>,
    /// Resampler: input frames per output frame (16.16), position
    /// between `prev` and the next input frame (16.16).
    step: u32,
    phase: u32,
    prev: [i32; 2],
    primed: bool,
    /// Bumped by Stop: writers blocked on the old queue give up.
    pub generation: u32,
}

impl Stream {
    pub fn frame_bytes(&self) -> usize {
        (self.channels * self.bits / 8) as usize
    }

    /// Input bytes waiting to be mixed.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Bytes the stream may hold before a writer waits (0.5 s).
    pub fn limit(&self) -> usize {
        self.rate as usize * self.frame_bytes() / 2
    }

    pub fn push(&mut self, data: &[u8]) {
        self.queue.extend(data.iter().copied());
        self.written += data.len() as u64;
    }

    /// Discard what is queued (Stop): only what was mixed still plays,
    /// and the position no longer waits for it.
    pub fn discard(&mut self) {
        self.queue.clear();
        self.written = self.consumed;
        self.played = self.consumed;
        self.marks.clear();
        self.primed = false;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Everything written has played (the last partial frame aside).
    pub fn drained(&self) -> bool {
        let fb = self.frame_bytes() as u64;
        self.played >= self.written - self.written % fb.max(1)
    }

    /// Input frame `i` of the queue as 16-bit stereo.
    fn frame(&self, i: usize) -> [i32; 2] {
        let fb = self.frame_bytes();
        let at = i * fb;
        let s = |k: usize| -> i32 {
            if self.bits == 8 {
                (self.queue[at + k] as i32 - 128) << 8
            } else {
                i16::from_le_bytes([self.queue[at + 2 * k], self.queue[at + 2 * k + 1]]) as i32
            }
        };
        let l = s(0);
        [l, if self.channels == 2 { s(1) } else { l }]
    }

    fn pop_frame(&mut self) -> Option<[i32; 2]> {
        let fb = self.frame_bytes();
        if self.queue.len() < fb {
            return None;
        }
        let f = self.frame(0);
        self.queue.drain(..fb);
        self.consumed += fb as u64;
        Some(f)
    }

    /// Add up to `acc.len() / 2` output frames, scaled by the volume.
    fn mix(&mut self, acc: &mut [i32]) {
        if !self.primed {
            let Some(f) = self.pop_frame() else { return };
            self.prev = f;
            self.phase = 0;
            self.primed = true;
        }
        let vol = self.volume as i32;
        let fb = self.frame_bytes();
        for o in acc.chunks_exact_mut(2) {
            let next = if self.queue.len() >= fb { self.frame(0) } else { self.prev };
            let p = self.phase as i64;
            for ch in 0..2 {
                let a = self.prev[ch] as i64;
                let v = a + ((next[ch] as i64 - a) * p >> 16);
                o[ch] += v as i32 * vol;
            }
            self.phase += self.step;
            while self.phase >= 1 << 16 {
                match self.pop_frame() {
                    Some(f) => {
                        self.prev = f;
                        self.phase -= 1 << 16;
                    }
                    None => {
                        // Played out: start afresh with the next write.
                        self.primed = false;
                        return;
                    }
                }
            }
        }
    }
}

/// A sound driver's output: a ring of [`RING_FRAMES`] 16-bit stereo
/// frames the hardware plays in a loop at [`Output::rate`].
pub enum Output {
    Hda(hda::Output),
    Ac97(ac97::Output),
    Es137x(es137x::Output),
}

macro_rules! each {
    ($s:expr, $o:ident => $e:expr) => {
        match $s {
            Output::Hda($o) => $e,
            Output::Ac97($o) => $e,
            Output::Es137x($o) => $e,
        }
    };
}

impl Output {
    /// The ring of interleaved stereo samples.
    pub fn ring(&mut self) -> &mut [i16] {
        each!(self, o => o.ring())
    }

    /// The frame the hardware plays next.
    pub fn position(&self) -> usize {
        each!(self, o => o.position())
    }

    pub fn running(&self) -> bool {
        each!(self, o => o.running())
    }

    /// Frames per second.
    pub fn rate(&self) -> u32 {
        each!(self, o => o.rate())
    }

    /// Start playing the ring from frame 0 (the caller has filled it).
    pub fn start(&mut self) {
        each!(self, o => o.start())
    }

    pub fn stop(&mut self) {
        each!(self, o => o.stop())
    }

    /// The driver's registers, for the log.
    pub fn diag(&self) -> alloc::string::String {
        each!(self, o => o.diag())
    }
}

pub struct Mixer {
    out: Option<Output>,
    /// The hardware's rate.
    rate: u32,
    pub streams: Vec<Stream>,
    pub master: u32,
    next_id: u32,
    /// Ring frames played (the hardware's next frame) and filled, counted
    /// from the start of the stream.
    hw: u64,
    filled: u64,
    last_pos: usize,
    idle_since: u64,
    /// When the hardware position last moved.
    moved_at: u64,
    /// Software pacing (tick and ring frame it started at), once the
    /// hardware position has stood still for [`STALL_MS`] while running.
    soft: Option<(u64, u64)>,
}

pub static MIXER: SpinLock<Mixer> = SpinLock::new(Mixer {
    out: None,
    rate: 48000,
    streams: Vec::new(),
    master: 100,
    next_id: 1,
    hw: 0,
    filled: 0,
    last_pos: 0,
    idle_since: 0,
    moved_at: 0,
    soft: None,
});

static PRESENT: AtomicBool = AtomicBool::new(false);

/// Writers and drainers waiting for the mixer to make progress.
pub static WAITERS: WaitQueue = WaitQueue::new();

/// Whether there is sound hardware.
pub fn present() -> bool {
    PRESENT.load(Ordering::Acquire)
}

/// The driver found an output: use it (the first one).
pub fn attach(out: Output) {
    let mut m = MIXER.lock();
    if m.out.is_none() {
        m.rate = out.rate();
        m.out = Some(out);
        PRESENT.store(true, Ordering::Release);
    }
}

impl Mixer {
    pub fn stream(&mut self, id: u32, pid: u32) -> Option<&mut Stream> {
        self.streams.iter_mut().find(|s| s.id == id && s.pid == pid)
    }

    /// A new stream; `None` when there are too many.
    pub fn open(&mut self, pid: u32, rate: u32, channels: u32, bits: u32) -> Option<u32> {
        if self.streams.len() >= MAX_STREAMS {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.streams.push(Stream {
            id,
            pid,
            rate,
            channels,
            bits,
            queue: VecDeque::new(),
            volume: 100,
            paused: false,
            written: 0,
            consumed: 0,
            played: 0,
            marks: VecDeque::new(),
            step: ((((rate as u64) << 16) + self.rate as u64 / 2) / self.rate as u64) as u32,
            phase: 0,
            prev: [0; 2],
            primed: false,
            generation: 0,
        });
        Some(id)
    }

    pub fn close(&mut self, id: u32, pid: u32) -> bool {
        let n = self.streams.len();
        self.streams.retain(|s| !(s.id == id && s.pid == pid));
        self.streams.len() != n
    }

    /// How far ahead of the hardware the ring is filled, in frames.
    fn lead(&self) -> u64 {
        self.rate as u64 * LEAD_MS / 1000
    }

    /// Mix ring frames from `filled` up to `until`.
    fn fill(&mut self, until: u64) {
        let master = self.master as i64;
        let Some(out) = self.out.as_mut() else { return };
        let ring = out.ring();
        let mut acc = [0i32; CHUNK * 2];
        while self.filled < until {
            let off = (self.filled % RING_FRAMES as u64) as usize;
            let n = ((until - self.filled) as usize).min(CHUNK).min(RING_FRAMES - off);
            let acc = &mut acc[..n * 2];
            acc.fill(0);
            let end = self.filled + n as u64;
            for s in self.streams.iter_mut().filter(|s| !s.paused) {
                let before = s.consumed;
                s.mix(acc);
                if s.consumed != before {
                    s.marks.push_back((end, s.consumed));
                }
            }
            for (d, &a) in ring[off * 2..(off + n) * 2].iter_mut().zip(acc.iter()) {
                *d = (a as i64 * master / 10000).clamp(-32768, 32767) as i16;
            }
            self.filled = end;
        }
    }

    /// Account for what the hardware has played.
    fn advance(&mut self, now: u64) {
        let Some(out) = self.out.as_ref() else { return };
        let hz = crate::irq::HZ;
        if let Some((t0, f0)) = self.soft {
            self.hw = self.hw.max(f0 + (now - t0) * self.rate as u64 / hz);
        } else {
            let pos = out.position();
            let delta = (pos + RING_FRAMES - self.last_pos) % RING_FRAMES;
            self.last_pos = pos;
            self.hw += delta as u64;
            if delta != 0 {
                self.moved_at = now;
            } else if now - self.moved_at >= STALL_MS * hz / 1000 {
                crate::println!(
                    "audio: the output's position stays at {pos} ({}); pacing by the clock from now on, without sound",
                    out.diag()
                );
                self.soft = Some((now, self.hw));
            }
        }
        let hw = self.hw;
        for s in self.streams.iter_mut() {
            while let Some(&(f, c)) = s.marks.front() {
                if f > hw {
                    break;
                }
                s.played = c;
                s.marks.pop_front();
            }
        }
    }

    fn start(&mut self, now: u64) {
        self.hw = 0;
        self.filled = 0;
        self.last_pos = 0;
        self.moved_at = now;
        if self.soft.is_some() {
            self.soft = Some((now, 0));
        }
        let Some(out) = self.out.as_mut() else { return };
        out.ring().fill(0);
        self.fill(self.lead());
        if let Some(out) = self.out.as_mut() {
            out.start();
        }
    }

    /// Returns whether waiters should look again.
    fn tick(&mut self, now: u64) -> bool {
        let Some(out) = self.out.as_ref() else { return false };
        if !out.running() {
            if self.streams.is_empty() {
                return false;
            }
            self.start(now);
            self.idle_since = now;
            return true;
        }
        self.advance(now);
        if self.streams.is_empty() {
            if now - self.idle_since >= IDLE_STOP_MS * crate::irq::HZ / 1000 {
                if let Some(o) = self.out.as_mut() {
                    o.stop();
                }
                return false;
            }
        } else {
            self.idle_since = now;
        }
        // Fell behind (the tick did not run): skip what was missed.
        if self.filled < self.hw + 16 {
            self.filled = self.hw + 16;
            for s in self.streams.iter_mut() {
                s.marks.clear();
                s.played = s.consumed;
            }
        }
        let until = self.hw + self.lead();
        if until < self.filled + self.rate as u64 * MIN_FILL_MS / 1000 {
            return false;
        }
        self.fill(until);
        true
    }
}

/// From the timer tick (boot processor, interrupt context).
pub fn tick() {
    if !present() {
        return;
    }
    let now = crate::irq::ticks();
    let woke = match MIXER.try_lock() {
        Some(mut m) => m.tick(now),
        None => false,
    };
    if woke {
        WAITERS.wake_all();
    }
}

/// A process ended: close its streams.
pub fn process_ended(pid: u32) {
    if !present() {
        return;
    }
    MIXER.lock().streams.retain(|s| s.pid != pid);
    WAITERS.wake_all();
}
