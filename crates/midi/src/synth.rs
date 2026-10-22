//! A small General MIDI software synthesizer: two-operator FM voices with
//! envelopes, one timbre per GM program family, synthesized drums on
//! channel 10, and the usual channel controls (volume, expression, pan,
//! sustain, pitch bend, program change). Output is 16-bit stereo.
//!
//! No floating-point library is needed: sines come from a table and
//! envelopes decay by multiplication.

use alloc::vec::Vec;

use crate::smf::Message;

const TABLE: usize = 2048;
const MAX_VOICES: usize = 32;

/// sin over one cycle, TABLE entries (filled at start).
fn sine_table() -> Vec<f32> {
    let mut t = Vec::with_capacity(TABLE);
    for i in 0..TABLE {
        // Taylor series around the nearest multiple of pi/2.
        let x = i as f32 / TABLE as f32 * 2.0 * core::f32::consts::PI;
        t.push(sin(x));
    }
    t
}

fn sin(x: f32) -> f32 {
    use core::f32::consts::PI;
    // Reduce to [-pi, pi], then to [-pi/2, pi/2].
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    }
    if x > PI / 2.0 {
        x = PI - x;
    } else if x < -PI / 2.0 {
        x = -PI - x;
    }
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
}

/// 2^x for |x| up to a few octaves.
fn exp2(x: f32) -> f32 {
    let i = if x >= 0.0 { x as i32 } else { x as i32 - 1 };
    let f = x - i as f32;
    // 2^f on [0, 1): a short polynomial (error below 1e-4).
    let p = 1.0 + f * (0.693_147 + f * (0.240_227 + f * (0.055_504 + f * 0.009_618)));
    if i >= 0 { p * (1u32 << i.min(30)) as f32 } else { p / (1u32 << (-i).min(30)) as f32 }
}

/// A timbre: FM modulator ratio and depth, the depth's decay, and the
/// amplitude envelope (seconds; sustain level 0..1).
#[derive(Clone, Copy)]
struct Timbre {
    ratio: f32,
    index: f32,
    index_decay: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    /// Gain relative to the other timbres.
    gain: f32,
}

const fn t(ratio: f32, index: f32, index_decay: f32, attack: f32, decay: f32, sustain: f32, release: f32, gain: f32) -> Timbre {
    Timbre { ratio, index, index_decay, attack, decay, sustain, release, gain }
}

/// One timbre per GM family of eight programs.
const FAMILIES: [Timbre; 16] = [
    t(1.0, 2.0, 1.5, 0.002, 1.20, 0.25, 0.30, 1.0),  // piano
    t(3.5, 1.5, 3.0, 0.001, 0.60, 0.00, 0.30, 0.9),  // chromatic percussion
    t(1.0, 1.2, 0.0, 0.010, 0.10, 0.90, 0.08, 0.7),  // organ
    t(1.0, 2.5, 2.0, 0.002, 0.90, 0.15, 0.25, 0.9),  // guitar
    t(1.0, 1.8, 1.0, 0.004, 0.50, 0.50, 0.15, 1.1),  // bass
    t(1.0, 1.0, 0.0, 0.080, 0.30, 0.85, 0.30, 0.7),  // strings
    t(1.0, 0.9, 0.0, 0.120, 0.40, 0.80, 0.40, 0.7),  // ensemble
    t(1.0, 2.2, 0.5, 0.030, 0.20, 0.80, 0.15, 0.7),  // brass
    t(2.0, 1.4, 0.3, 0.030, 0.20, 0.80, 0.15, 0.7),  // reed
    t(1.0, 0.5, 0.0, 0.040, 0.20, 0.85, 0.15, 0.8),  // pipe
    t(1.0, 3.0, 0.5, 0.005, 0.30, 0.70, 0.15, 0.6),  // synth lead
    t(0.5, 1.5, 0.2, 0.200, 0.50, 0.80, 0.60, 0.6),  // synth pad
    t(1.4, 3.0, 1.0, 0.010, 0.80, 0.40, 0.50, 0.6),  // synth effects
    t(3.0, 2.0, 2.0, 0.002, 0.70, 0.10, 0.30, 0.8),  // ethnic
    t(1.7, 3.0, 4.0, 0.001, 0.25, 0.00, 0.10, 0.9),  // percussive
    t(0.7, 4.0, 1.0, 0.050, 0.50, 0.50, 0.40, 0.5),  // sound effects
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

/// Drum sounds: (start pitch Hz, pitch fall per second as a factor, noise
/// share 0..1, decay seconds).
fn drum_sound(note: u8) -> (f32, f32, f32, f32) {
    match note {
        35 | 36 => (110.0, 0.02, 0.05, 0.25),          // bass drum
        37 | 38 | 40 => (190.0, 0.3, 0.70, 0.18),      // snare, side stick
        39 => (300.0, 0.5, 0.90, 0.12),                // clap
        41 | 43 | 45 | 47 | 48 | 50 => (60.0 + (note as f32 - 40.0) * 12.0, 0.4, 0.15, 0.30), // toms
        42 | 44 => (6000.0, 1.0, 1.00, 0.05),          // closed hi-hat
        46 => (6000.0, 1.0, 1.00, 0.30),               // open hi-hat
        49 | 52 | 55 | 57 => (5000.0, 1.0, 1.00, 1.20), // crash
        51 | 53 | 59 => (4000.0, 1.0, 0.80, 0.80),     // ride
        _ => (400.0, 0.5, 0.60, 0.15),
    }
}

struct Voice {
    channel: u8,
    note: u8,
    drum: bool,
    /// Phase increments (cycles per sample) and phases.
    inc: f32,
    phase: f32,
    mod_phase: f32,
    timbre: Timbre,
    stage: Stage,
    env: f32,
    index: f32,
    velocity: f32,
    /// Held by the sustain pedal after its note-off.
    held: bool,
    /// Drums: pitch fall and noise share; noise state.
    fall: f32,
    noise: f32,
    rng: u32,
    age: u32,
}

#[derive(Clone, Copy)]
struct Channel {
    program: u8,
    volume: f32,
    expression: f32,
    pan: f32,
    sustain: bool,
    /// Pitch bend in semitones, and the bend range.
    bend: f32,
    bend_range: f32,
    rpn: (u8, u8),
}

impl Default for Channel {
    fn default() -> Self {
        Channel { program: 0, volume: 100.0 / 127.0, expression: 1.0, pan: 0.5, sustain: false, bend: 0.0, bend_range: 2.0, rpn: (127, 127) }
    }
}

pub struct Synth {
    rate: f32,
    sine: Vec<f32>,
    voices: Vec<Voice>,
    channels: [Channel; 16],
    pub master: f32,
}

impl Synth {
    pub fn new(rate: u32) -> Synth {
        Synth { rate: rate as f32, sine: sine_table(), voices: Vec::new(), channels: [Channel::default(); 16], master: 1.0 }
    }

    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    /// All notes off and the controllers reset.
    pub fn reset(&mut self) {
        self.voices.clear();
        self.channels = [Channel::default(); 16];
    }

    /// Release every sounding note (STOP, PAUSE).
    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.held = false;
            v.stage = Stage::Release;
        }
    }

    pub fn active(&self) -> usize {
        self.voices.len()
    }

    pub fn message(&mut self, m: Message) {
        let ch = m.channel() as usize;
        match m.kind() {
            0x80 => self.note_off(ch as u8, m.data1),
            0x90 if m.data2 == 0 => self.note_off(ch as u8, m.data1),
            0x90 => self.note_on(ch as u8, m.data1, m.data2),
            0xb0 => self.control(ch, m.data1, m.data2),
            0xc0 => self.channels[ch].program = m.data1,
            0xe0 => {
                let v = (m.data2 as i32) << 7 | m.data1 as i32;
                let c = &mut self.channels[ch];
                c.bend = (v - 8192) as f32 / 8192.0 * c.bend_range;
                let bend = c.bend;
                for v in self.voices.iter_mut().filter(|v| v.channel == ch as u8 && !v.drum) {
                    v.inc = note_freq(v.note, bend) / self.rate;
                }
            }
            _ => {}
        }
    }

    fn control(&mut self, ch: usize, cc: u8, val: u8) {
        let c = &mut self.channels[ch];
        match cc {
            7 => c.volume = val as f32 / 127.0,
            10 => c.pan = val as f32 / 127.0,
            11 => c.expression = val as f32 / 127.0,
            64 => {
                c.sustain = val >= 64;
                if !c.sustain {
                    for v in self.voices.iter_mut().filter(|v| v.channel == ch as u8 && v.held) {
                        v.held = false;
                        v.stage = Stage::Release;
                    }
                }
            }
            101 => c.rpn.0 = val,
            100 => c.rpn.1 = val,
            // Data entry for RPN 0: the pitch bend range in semitones.
            6 if c.rpn == (0, 0) => c.bend_range = val as f32,
            120 => self.voices.retain(|v| v.channel != ch as u8),
            121 => {
                *c = Channel { program: c.program, ..Channel::default() };
            }
            123 => {
                for v in self.voices.iter_mut().filter(|v| v.channel == ch as u8) {
                    v.stage = Stage::Release;
                }
            }
            _ => {}
        }
    }

    fn note_off(&mut self, ch: u8, note: u8) {
        let sustain = self.channels[ch as usize].sustain;
        for v in self.voices.iter_mut().filter(|v| v.channel == ch && v.note == note && v.stage != Stage::Release && !v.held) {
            if v.drum {
                continue;
            }
            if sustain {
                v.held = true;
            } else {
                v.stage = Stage::Release;
            }
        }
    }

    fn note_on(&mut self, ch: u8, note: u8, vel: u8) {
        let drum = ch == 9;
        let c = self.channels[ch as usize];
        // A note already sounding on this channel is retriggered.
        self.voices.retain(|v| !(v.channel == ch && v.note == note && !drum));
        if self.voices.len() >= MAX_VOICES {
            // Steal the oldest releasing voice, else the oldest.
            let i = self.voices.iter().enumerate().filter(|(_, v)| v.stage == Stage::Release).max_by_key(|(_, v)| v.age).map(|(i, _)| i);
            let i = i.unwrap_or_else(|| self.voices.iter().enumerate().max_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap());
            self.voices.swap_remove(i);
        }
        let (inc, timbre, fall, noise) = if drum {
            let (f, fall, noise, decay) = drum_sound(note);
            (f / self.rate, t(1.0, 0.0, 0.0, 0.001, decay, 0.0, decay, 1.0), fall, noise)
        } else {
            (note_freq(note, c.bend) / self.rate, FAMILIES[(c.program / 8) as usize], 0.0, 0.0)
        };
        self.voices.push(Voice {
            channel: ch,
            note,
            drum,
            inc,
            phase: 0.0,
            mod_phase: 0.0,
            timbre,
            stage: Stage::Attack,
            env: 0.0,
            index: timbre.index,
            velocity: vel as f32 / 127.0,
            held: false,
            fall,
            noise,
            rng: 0x1234_5678 ^ note as u32 * 0x9e37,
            age: 0,
        });
    }

    /// Add `frames` stereo frames to `out` (interleaved L, R), clipping.
    pub fn render(&mut self, out: &mut [i16]) {
        let frames = out.len() / 2;
        let rate = self.rate;
        let tsize = TABLE as f32;
        let mut mix = alloc::vec![0f32; frames * 2];
        for v in &mut self.voices {
            v.age = v.age.wrapping_add(1);
            let c = self.channels[v.channel as usize];
            let gain = v.velocity * v.velocity * c.volume * c.expression * v.timbre.gain * 0.25;
            let (gl, gr) = (gain * (1.0 - c.pan).min(0.5) * 2.0, gain * c.pan.min(0.5) * 2.0);
            let tm = v.timbre;
            let attack_step = 1.0 / (tm.attack * rate).max(1.0);
            let decay_k = 1.0 - 1.0 / (tm.decay * rate * 0.25).max(1.0);
            let release_k = 1.0 - 1.0 / (tm.release * rate * 0.25).max(1.0);
            let index_k = if tm.index_decay > 0.0 { 1.0 - tm.index_decay / rate } else { 1.0 };
            let fall_k = if v.drum && v.fall < 1.0 { 1.0 - (1.0 - v.fall) * 8.0 / rate } else { 1.0 };
            for f in 0..frames {
                match v.stage {
                    Stage::Attack => {
                        v.env += attack_step;
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        v.env = tm.sustain + (v.env - tm.sustain) * decay_k;
                        if v.env - tm.sustain < 0.001 {
                            v.stage = if tm.sustain <= 0.001 { Stage::Off } else { Stage::Sustain };
                        }
                    }
                    Stage::Sustain => {}
                    Stage::Release => {
                        v.env *= release_k;
                        if v.env < 0.0005 {
                            v.stage = Stage::Off;
                        }
                    }
                    Stage::Off => break,
                }
                let s = if v.drum {
                    v.rng ^= v.rng << 13;
                    v.rng ^= v.rng >> 17;
                    v.rng ^= v.rng << 5;
                    let n = (v.rng as i32 as f32) / i32::MAX as f32;
                    let tone = self.sine[(v.phase * tsize) as usize % TABLE];
                    v.phase = (v.phase + v.inc) % 1.0;
                    v.inc *= fall_k;
                    tone * (1.0 - v.noise) + n * v.noise
                } else {
                    let m = self.sine[(v.mod_phase * tsize) as usize % TABLE] * v.index;
                    let p = v.phase + m / (2.0 * core::f32::consts::PI);
                    let p = p - p.floor_();
                    v.phase = (v.phase + v.inc) % 1.0;
                    v.mod_phase = (v.mod_phase + v.inc * tm.ratio) % 1.0;
                    v.index *= index_k;
                    self.sine[(p * tsize) as usize % TABLE]
                };
                let s = s * v.env;
                mix[f * 2] += s * gl;
                mix[f * 2 + 1] += s * gr;
            }
        }
        self.voices.retain(|v| v.stage != Stage::Off);
        let m = self.master * 32767.0;
        for (o, s) in out.iter_mut().zip(mix) {
            let x = *o as f32 + s * m;
            *o = x.clamp(-32768.0, 32767.0) as i16;
        }
    }
}

trait Floor {
    fn floor_(self) -> f32;
}

impl Floor for f32 {
    fn floor_(self) -> f32 {
        let i = self as i32 as f32;
        if i > self { i - 1.0 } else { i }
    }
}

/// The frequency of a MIDI note with a pitch bend in semitones.
pub fn note_freq(note: u8, bend: f32) -> f32 {
    440.0 * exp2((note as f32 - 69.0 + bend) / 12.0)
}
