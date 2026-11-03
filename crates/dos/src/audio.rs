//! The machine's sound output: one 16-bit stereo stream at [`RATE`] that
//! mixes the PC speaker, the Sound Blaster's DSP (DMA and direct DAC),
//! its FM synthesizer (crate::opl) and a General MIDI synthesizer behind
//! an MPU-401 in UART mode (port 330h, crate `midi`).
//!
//! Output time is the machine's clock: between instruction slices the
//! frames due since the last mix are rendered and handed to the host
//! (Host::audio), and the Sound Blaster takes its DMA samples as those
//! frames pass, so its block interrupts come at the programmed rate.
//! Register and port writes made during a slice take effect at the
//! slice's frames; the direct DAC's samples and the speaker's level
//! changes written in a slice are spread over its frames, which keeps
//! their pitch. Nothing is sent while every source is quiet.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// Output frames per second.
pub const RATE: u32 = 22050;
/// The speaker's square wave amplitude.
const SPEAKER_AMP: i32 = 5000;

/// An MPU-401 in UART mode feeding a General MIDI synthesizer.
pub struct Mpu {
    out: VecDeque<u8>,
    uart: bool,
    /// The message being collected (running status kept).
    status: u8,
    data: [u8; 2],
    have: usize,
    in_sysex: bool,
    pub synth: Option<midi::Synth>,
}

impl Default for Mpu {
    fn default() -> Self {
        Mpu { out: VecDeque::new(), uart: false, status: 0, data: [0; 2], have: 0, in_sysex: false, synth: None }
    }
}

impl Mpu {
    pub fn io_read(&mut self, port: u16) -> u8 {
        if port & 1 == 0 {
            self.out.pop_front().unwrap_or(0xff)
        } else {
            // Bit 6 clear: ready for output; bit 7 clear: data to read.
            if self.out.is_empty() { 0x80 } else { 0x00 }
        }
    }

    pub fn io_write(&mut self, port: u16, v: u8) {
        if port & 1 != 0 {
            match v {
                0xff => {
                    // Reset: acknowledged except when leaving UART mode.
                    if !self.uart {
                        self.out.push_back(0xfe);
                    }
                    self.uart = false;
                    if let Some(s) = &mut self.synth {
                        s.all_notes_off();
                    }
                }
                0x3f => {
                    self.uart = true;
                    self.out.push_back(0xfe);
                }
                _ => {
                    if !self.uart {
                        self.out.push_back(0xfe);
                    }
                }
            }
            return;
        }
        self.byte(v);
    }

    fn byte(&mut self, v: u8) {
        if v >= 0xf8 {
            return;
        }
        if v == 0xf0 {
            self.in_sysex = true;
            return;
        }
        if self.in_sysex {
            if v == 0xf7 || v & 0x80 != 0 {
                self.in_sysex = false;
            }
            if v & 0x80 == 0 || v == 0xf7 {
                return;
            }
        }
        if v & 0x80 != 0 {
            self.status = v;
            self.have = 0;
            return;
        }
        if self.status < 0x80 || self.status >= 0xf0 {
            return;
        }
        self.data[self.have] = v;
        self.have += 1;
        let need = if matches!(self.status & 0xf0, 0xc0 | 0xd0) { 1 } else { 2 };
        if self.have < need {
            return;
        }
        self.have = 0;
        let m = midi::smf::Message { status: self.status, data1: self.data[0], data2: if need == 2 { self.data[1] } else { 0 } };
        self.synth.get_or_insert_with(|| midi::Synth::new(RATE)).message(m);
    }
}

/// The mixer's state between slices.
#[derive(Default)]
pub struct Audio {
    /// Output time reached (microseconds) and the fraction of a frame.
    last_us: Option<u64>,
    acc: u64,
    /// Sound until then (microseconds), so the tail of a sound and a
    /// following silence reach the host before output stops.
    active_until: u64,
    spk_phase: u32,
    /// Port 61h speaker levels written during the slice (gate off).
    pub spk_levels: VecDeque<bool>,
    spk_level: bool,
    dac_level: i32,
    mix: Vec<[i32; 2]>,
    out: Vec<i16>,
    midi: Vec<i16>,
}

impl crate::Machine {
    /// Render the frames due since the last call and give them to the
    /// host.
    pub(crate) fn audio_tick<H: crate::Host>(&mut self, host: &mut H) {
        let now = self.hw.us_now;
        let a = &mut self.audio;
        let Some(last) = a.last_us.filter(|&l| l <= now) else {
            a.last_us = Some(now);
            return;
        };
        let dt = (now - last).min(100_000);
        a.last_us = Some(now);
        a.acc += dt * RATE as u64;
        let n = (a.acc / 1_000_000) as usize;
        a.acc %= 1_000_000;
        if n == 0 {
            return;
        }
        let spk_hz = self.hw.speaker().or((now < self.beep_until).then_some(896));
        let sb_on = self.hw.sb.x.on && !self.hw.sb.x.paused;
        let midi_on = self.hw.mpu.synth.as_ref().is_some_and(|s| s.active() > 0);
        let busy = spk_hz.is_some() || sb_on || self.hw.opl.active() || midi_on || !self.audio.spk_levels.is_empty() || !self.hw.sb.dac.is_empty();
        if busy {
            self.audio.active_until = now + 300_000;
        } else if now >= self.audio.active_until {
            self.audio.dac_level = 0;
            return;
        }
        let mut mix = core::mem::take(&mut self.audio.mix);
        mix.clear();
        mix.resize(n, [0, 0]);
        // FM and voice through the mixer's volumes.
        let fm_vol = self.hw.sb.gain(0x34);
        let voice = self.hw.sb.gain(0x32);
        self.hw.opl.clock = self.hw.opl.clock.max(now);
        self.hw.opl.render(&mut mix, [fm_vol[0] * 2, fm_vol[1] * 2]);
        if midi_on {
            let a = &mut self.audio;
            a.midi.clear();
            a.midi.resize(n * 2, 0);
            if let Some(s) = &mut self.hw.mpu.synth {
                s.render(&mut a.midi);
            }
            for (f, m) in mix.iter_mut().zip(a.midi.chunks(2)) {
                f[0] += m[0] as i32;
                f[1] += m[1] as i32;
            }
        }
        // The DSP.
        if sb_on {
            let rate = self.sb_rate();
            for f in mix.iter_mut() {
                let sb = &mut self.hw.sb;
                sb.frac += rate;
                while self.hw.sb.frac >= RATE {
                    self.hw.sb.frac -= RATE;
                    self.hw.sb.cur = self.sb_frame();
                }
                let c = self.hw.sb.cur;
                f[0] += c[0] * voice[0] >> 8;
                f[1] += c[1] * voice[1] >> 8;
            }
            if !self.hw.sb.x.on {
                self.hw.sb.cur = [0, 0];
            }
        }
        let dac = core::mem::take(&mut self.hw.sb.dac);
        let a = &mut self.audio;
        if !dac.is_empty() || a.dac_level != 0 {
            let k = dac.len();
            for (i, f) in mix.iter_mut().enumerate() {
                if k > 0 {
                    a.dac_level = (dac[i * k / n] as i32 - 0x80) << 8;
                }
                f[0] += a.dac_level * voice[0] >> 8;
                f[1] += a.dac_level * voice[1] >> 8;
            }
        }
        // The speaker: PIT channel 2's square wave, or the levels the
        // program put on port 61h itself.
        let levels = core::mem::take(&mut a.spk_levels);
        if let Some(hz) = spk_hz {
            for f in mix.iter_mut() {
                a.spk_phase = a.spk_phase.wrapping_add(hz * 2);
                let v = if (a.spk_phase / RATE) % 2 == 0 { SPEAKER_AMP } else { -SPEAKER_AMP };
                f[0] += v;
                f[1] += v;
            }
        } else if !levels.is_empty() {
            let k = levels.len();
            for (i, f) in mix.iter_mut().enumerate() {
                a.spk_level = levels[i * k / n];
                let v = if a.spk_level { SPEAKER_AMP } else { -SPEAKER_AMP };
                f[0] += v;
                f[1] += v;
            }
        }
        a.out.clear();
        a.out.extend(mix.iter().flat_map(|f| [f[0].clamp(-32768, 32767) as i16, f[1].clamp(-32768, 32767) as i16]));
        a.mix = mix;
        host.audio(RATE, &a.out);
        self.sb_irq();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpu_uart() {
        let mut m = Mpu::default();
        m.io_write(0x331, 0xff);
        assert_eq!(m.io_read(0x331) & 0x80, 0);
        assert_eq!(m.io_read(0x330), 0xfe);
        m.io_write(0x331, 0x3f);
        assert_eq!(m.io_read(0x330), 0xfe);
        assert_eq!(m.io_read(0x331) & 0xc0, 0x80);
        // Note on, running status, note off by velocity 0.
        for b in [0x90, 60, 100, 64, 100] {
            m.io_write(0x330, b);
        }
        assert_eq!(m.synth.as_ref().unwrap().active(), 2);
    }
}
