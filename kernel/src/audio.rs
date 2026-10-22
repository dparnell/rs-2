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
//!
//! Sound input is one [`Capture`] stream at a time. The driver's
//! [`Input`] records 16-bit stereo into a ring of [`RING_FRAMES`] frames
//! at its own rate (48 kHz, 44.1 kHz on the ES1370); the tick takes what
//! the hardware has written since the last tick, scales it by the
//! stream's volume (the recording level), resamples it to the stream's
//! rate and converts it to its format, and queues it for Rs2AudioRead
//! (at most [`CAPTURE_SECS`]; the oldest goes when a reader falls
//! behind). An input whose position stands still for [`STALL_MS`] is
//! replaced by silence at the clock's pace, as for output.

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
/// Most a capture stream holds for its reader (seconds).
const CAPTURE_SECS: usize = 2;
/// Input frames left in the ring until the next tick: the hardware's
/// position may run a little ahead of what its DMA has stored.
const IN_GUARD: usize = 16;

/// Input sources ([`Input::set_source`]).
pub const SOURCE_LINE: u32 = 1;
pub const SOURCE_MIC: u32 = 2;

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

/// A sound driver's input: a ring of [`RING_FRAMES`] 16-bit stereo
/// frames the hardware records into in a loop at [`Input::rate`].
pub enum Input {
    Hda(hda::Input),
    Ac97(ac97::Input),
    Es137x(es137x::Input),
}

macro_rules! each_in {
    ($s:expr, $o:ident => $e:expr) => {
        match $s {
            Input::Hda($o) => $e,
            Input::Ac97($o) => $e,
            Input::Es137x($o) => $e,
        }
    };
}

impl Input {
    /// The ring of interleaved stereo samples.
    pub fn ring(&self) -> &[i16] {
        each_in!(self, o => o.ring())
    }

    /// The frame the hardware records next.
    pub fn position(&self) -> usize {
        each_in!(self, o => o.position())
    }

    pub fn running(&self) -> bool {
        each_in!(self, o => o.running())
    }

    pub fn rate(&self) -> u32 {
        each_in!(self, o => o.rate())
    }

    /// Start recording into the ring from frame 0.
    pub fn start(&mut self) {
        each_in!(self, o => o.start())
    }

    pub fn stop(&mut self) {
        each_in!(self, o => o.stop())
    }

    /// The [`SOURCE_LINE`] and [`SOURCE_MIC`] bits the hardware has.
    pub fn sources(&self) -> u32 {
        each_in!(self, o => o.sources())
    }

    /// Record from `source` (or the closest it has); the source taken.
    pub fn set_source(&mut self, source: u32) -> u32 {
        each_in!(self, o => o.set_source(source))
    }

    pub fn diag(&self) -> alloc::string::String {
        each_in!(self, o => o.diag())
    }
}

/// The capture stream: what the input recorded, in its reader's format.
pub struct Capture {
    pub id: u32,
    pub pid: u32,
    pub rate: u32,
    channels: u32,
    bits: u32,
    queue: VecDeque<u8>,
    /// Bytes recorded since the stream opened (read, queued or dropped).
    pub captured: u64,
    /// The recording level, 0-100 (linear).
    pub volume: u32,
    /// Paused: what the input records is dropped.
    pub paused: bool,
    /// Bumped by Stop: readers waiting give up.
    pub generation: u32,
    /// Resampler: output frames per input frame inverted (input frames
    /// per output frame, 16.16) and the position between `prev` and the
    /// newest input frame.
    step: u32,
    phase: u32,
    prev: [i32; 2],
    primed: bool,
    /// Peak sample values, decaying (left, right).
    pub level: [i32; 2],
    level_frames: u32,
}

impl Capture {
    pub fn frame_bytes(&self) -> usize {
        (self.channels * self.bits / 8) as usize
    }

    /// Bytes waiting for the reader.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// The most the queue holds.
    pub fn limit(&self) -> usize {
        self.rate as usize * self.frame_bytes() * CAPTURE_SECS
    }

    /// Take up to `n` bytes (whole frames).
    pub fn take(&mut self, n: usize) -> Vec<u8> {
        let fb = self.frame_bytes();
        let n = n.min(self.queue.len());
        self.queue.drain(..n - n % fb).collect()
    }

    /// Stop: what is queued goes, waiting readers return.
    pub fn discard(&mut self) {
        self.queue.clear();
        self.primed = false;
        self.generation = self.generation.wrapping_add(1);
    }

    fn emit(&mut self, f: [i32; 2]) {
        let s = if self.channels == 1 { [(f[0] + f[1]) / 2, 0] } else { f };
        for &v in &s[..self.channels as usize] {
            if self.bits == 8 {
                self.queue.push_back(((v >> 8) + 128) as u8);
            } else {
                self.queue.extend((v as i16).to_le_bytes());
            }
        }
        self.captured += self.frame_bytes() as u64;
    }

    /// One input frame (16-bit stereo at the input's rate).
    fn feed(&mut self, f: [i16; 2]) {
        let vol = self.volume as i32;
        let f = [f[0] as i32 * vol / 100, f[1] as i32 * vol / 100];
        for ch in 0..2 {
            self.level[ch] = self.level[ch].max(f[ch].abs());
        }
        // About every 10 ms (at 48 kHz) the level falls by a fifth.
        self.level_frames += 1;
        if self.level_frames >= 480 {
            self.level_frames = 0;
            self.level = [self.level[0] * 4 / 5, self.level[1] * 4 / 5];
        }
        if self.paused {
            self.primed = false;
            return;
        }
        if !self.primed {
            self.prev = f;
            self.phase = 0;
            self.primed = true;
        }
        while self.phase < 1 << 16 {
            let p = self.phase as i64;
            let mut o = [0i32; 2];
            for ch in 0..2 {
                let a = self.prev[ch] as i64;
                o[ch] = (a + ((f[ch] as i64 - a) * p >> 16)) as i32;
            }
            self.emit(o);
            self.phase += self.step;
        }
        self.phase -= 1 << 16;
        self.prev = f;
    }

    /// Drop the oldest beyond [`Capture::limit`].
    fn trim(&mut self) {
        let (limit, fb) = (self.limit(), self.frame_bytes());
        if self.queue.len() > limit {
            let over = self.queue.len() - limit;
            self.queue.drain(..over.div_ceil(fb) * fb);
        }
    }
}

pub struct Mixer {
    out: Option<Output>,
    inp: Option<Input>,
    pub cap: Option<Capture>,
    /// The ring frame the input writes next as of the last tick, when it
    /// last moved, and the clock pacing that replaces a stuck input.
    in_last: usize,
    in_moved: u64,
    in_soft: Option<(u64, u64)>,
    /// Input frames taken since the capture began.
    in_frames: u64,
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
    inp: None,
    cap: None,
    in_last: 0,
    in_moved: 0,
    in_soft: None,
    in_frames: 0,
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
static INPUT: AtomicBool = AtomicBool::new(false);

/// Writers and drainers waiting for the mixer to make progress.
pub static WAITERS: WaitQueue = WaitQueue::new();

/// Whether there is sound hardware.
pub fn present() -> bool {
    PRESENT.load(Ordering::Acquire)
}

/// Whether the sound hardware records.
pub fn has_input() -> bool {
    INPUT.load(Ordering::Acquire)
}

/// The driver found an output (and maybe an input): use it (the first
/// one).
pub fn attach(out: Output, inp: Option<Input>) {
    let mut m = MIXER.lock();
    if m.out.is_none() {
        m.rate = out.rate();
        m.out = Some(out);
        PRESENT.store(true, Ordering::Release);
        if inp.is_some() {
            m.inp = inp;
            INPUT.store(true, Ordering::Release);
        }
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
        if self.streams.len() != n {
            return true;
        }
        if self.capture(id, pid).is_some() {
            self.close_input();
            return true;
        }
        false
    }

    /// The capture stream `id` of process `pid`.
    pub fn capture(&mut self, id: u32, pid: u32) -> Option<&mut Capture> {
        self.cap.as_mut().filter(|c| c.id == id && c.pid == pid)
    }

    /// Open the capture stream and start the input. `Err(true)` when
    /// another capture is open, `Err(false)` without input hardware.
    pub fn open_input(&mut self, pid: u32, rate: u32, channels: u32, bits: u32) -> Result<u32, bool> {
        if self.cap.is_some() {
            return Err(true);
        }
        let Some(inp) = self.inp.as_mut() else { return Err(false) };
        let in_rate = inp.rate();
        inp.start();
        let now = crate::irq::ticks();
        self.in_last = 0;
        self.in_moved = now;
        self.in_soft = None;
        self.in_frames = 0;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.cap = Some(Capture {
            id,
            pid,
            rate,
            channels,
            bits,
            queue: VecDeque::new(),
            captured: 0,
            volume: 100,
            paused: false,
            generation: 0,
            step: ((((in_rate as u64) << 16) + rate as u64 / 2) / rate as u64) as u32,
            phase: 0,
            prev: [0; 2],
            primed: false,
            level: [0; 2],
            level_frames: 0,
        });
        Ok(id)
    }

    fn close_input(&mut self) {
        self.cap = None;
        if let Some(inp) = self.inp.as_mut() {
            inp.stop();
        }
    }

    /// Select the input source; the one taken (0 without input).
    pub fn set_source(&mut self, source: u32) -> u32 {
        self.inp.as_mut().map_or(0, |i| i.set_source(source))
    }

    /// The sources the input has.
    pub fn sources(&self) -> u32 {
        self.inp.as_ref().map_or(0, |i| i.sources())
    }

    /// Take what the input has recorded since the last tick; whether
    /// anything came.
    fn capture_tick(&mut self, now: u64) -> bool {
        let (Some(inp), Some(cap)) = (self.inp.as_ref(), self.cap.as_mut()) else { return false };
        if !inp.running() {
            return false;
        }
        let hz = crate::irq::HZ;
        let rate = inp.rate() as u64;
        if let Some((t0, f0)) = self.in_soft {
            let due = f0 + (now - t0) * rate / hz;
            let n = due.saturating_sub(self.in_frames);
            for _ in 0..n {
                cap.feed([0, 0]);
            }
            self.in_frames += n;
            cap.trim();
            return n != 0;
        }
        let pos = inp.position();
        let avail = (pos + RING_FRAMES - self.in_last) % RING_FRAMES;
        if avail == 0 {
            if now - self.in_moved >= STALL_MS * hz / 1000 {
                crate::println!(
                    "audio: the input's position stays at {pos} ({}); recording silence by the clock from now on",
                    inp.diag()
                );
                self.in_soft = Some((now, self.in_frames));
            }
            return false;
        }
        self.in_moved = now;
        let n = avail.saturating_sub(IN_GUARD);
        if n == 0 {
            return false;
        }
        let ring = inp.ring();
        for k in 0..n {
            let i = (self.in_last + k) % RING_FRAMES;
            cap.feed([ring[i * 2], ring[i * 2 + 1]]);
        }
        self.in_last = (self.in_last + n) % RING_FRAMES;
        self.in_frames += n as u64;
        cap.trim();
        true
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
        let took = self.capture_tick(now);
        self.output_tick(now) || took
    }

    fn output_tick(&mut self, now: u64) -> bool {
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
    let mut m = MIXER.lock();
    m.streams.retain(|s| s.pid != pid);
    if m.cap.as_ref().is_some_and(|c| c.pid == pid) {
        m.close_input();
    }
    drop(m);
    WAITERS.wake_all();
}
