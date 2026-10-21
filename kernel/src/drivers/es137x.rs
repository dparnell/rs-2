//! Ensoniq AudioPCI: the ES1370 (QEMU's `ES1370`) and the ES1371 and
//! its successors, Creative's CT5880 and Ectiva's EV1938 (VMware's
//! "Sound Blaster" / `es1371` sound card).
//!
//! Output only, through DAC2 (the "voice" DAC; DAC1 is the synthesiser
//! channel). DAC2 plays a ring of [`RING_FRAMES`] 16-bit stereo frames
//! from memory page 0xc's frame registers and loops over it by itself;
//! its current count (in longwords, one frame each) is the position. The
//! serial interface's interrupts stay off: the mixer fills the ring from
//! the timer tick.
//!
//! The ES1370 has an AK4531 mixer codec (write only) and makes DAC2's
//! rate by dividing 1411200 Hz: 44100 Hz exactly. The ES1371 has an
//! AC'97 codec behind its codec register and a sample rate converter,
//! programmed for 48000 Hz on DAC2.

use super::pci;
use crate::arch::{inl, outl, outw};
use crate::audio::RING_FRAMES;
use crate::mm::phys;
use crate::msi;

const RING_BYTES: usize = RING_FRAMES * 4;

// Registers (I/O BAR 0).
const CONTROL: u16 = 0x00;
const STATUS: u16 = 0x04;
const MEM_PAGE: u16 = 0x0c;
const CODEC_1370: u16 = 0x10;
const SRC_1371: u16 = 0x10;
const CODEC_1371: u16 = 0x14;
const LEGACY_1371: u16 = 0x18;
const SERIAL: u16 = 0x20;
const DAC2_COUNT: u16 = 0x28;
// Memory page 0xc (DAC frames).
const PAGE_DAC: u32 = 0xc;
const DAC2_FRAME: u16 = 0x38;
const DAC2_SIZE: u16 = 0x3c;

// CONTROL.
const DAC2_EN: u32 = 1 << 5;
const CDC_EN_1370: u32 = 1 << 1;
const SERR_DIS_1370: u32 = 1 << 0;
const fn pclkdiv_1370(rate: u32) -> u32 {
    (1_411_200 / rate - 2) << 16
}
/// ES1371: AC'97 warm reset (starts the bit clock).
const SYNC_RES_1371: u32 = 1 << 14;

// STATUS.
const CSTAT_1370: u32 = 1 << 10;
const AC97_RST_1371: u32 = 1 << 29;

// SERIAL: DAC2 16-bit stereo, end increment 2, looping, not paused.
const P2_MODE_16_STEREO: u32 = 3 << 2;
const P2_END_INC: u32 = 2 << 19;
const P2_PAUSE: u32 = 1 << 12;

// ES1371 sample rate converter.
const SRC_WE: u32 = 1 << 24;
const SRC_BUSY: u32 = 1 << 23;
const SRC_DISABLE: u32 = 1 << 22;
const SRC_DIS_P1: u32 = 1 << 21;
const SRC_DIS_P2: u32 = 1 << 20;
const SRC_DIS_R1: u32 = 1 << 19;
const SRC_KEEP: u32 = SRC_DISABLE | SRC_DIS_P1 | SRC_DIS_P2 | SRC_DIS_R1;
const SR_DAC1: u32 = 0x70;
const SR_DAC2: u32 = 0x74;
const SR_ADC: u32 = 0x78;
const SR_VOL_ADC: u32 = 0x6c;
const SR_VOL_DAC1: u32 = 0x7c;
const SR_VOL_DAC2: u32 = 0x7e;
const SR_TRUNC_N: u32 = 0;
const SR_INT_REGS: u32 = 1;
const SR_VFREQ_FRAC: u32 = 3;

// ES1371 AC'97 codec register.
const CODEC_RDY: u32 = 1 << 31;
const CODEC_WIP: u32 = 1 << 30;
const CODEC_PIRD: u32 = 1 << 23;
/// EV1938: set in every codec access.
const CODEC_EV1938: u32 = 1 << 26;

// AC'97 mixer registers.
const AC_RESET: u32 = 0x00;
const AC_MASTER: u32 = 0x02;
const AC_HEADPHONE: u32 = 0x04;
const AC_PCM_OUT: u32 = 0x18;
const AC_POWERDOWN: u32 = 0x26;
const AC_VENDOR1: u32 = 0x7c;
const AC_VENDOR2: u32 = 0x7e;

/// Polls of a busy bit before giving up (each an I/O read, about a
/// microsecond).
const SPINS: u32 = 0xa000;

#[derive(Clone, Copy, PartialEq)]
enum Chip {
    Es1370,
    Es1371,
}

/// AudioPCI chips: the ES1370, or one with the ES1371's registers.
pub fn is_es137x(dev: &pci::Device) -> bool {
    matches!((dev.vendor, dev.device), (0x1274, 0x5000 | 0x1371 | 0x5880) | (0x1102, 0x8938))
}

/// DAC2: the ring the mixer fills.
pub struct Output {
    io: u16,
    ctrl: u32,
    ring: u64,
    rate: u32,
    running: bool,
}

impl Output {
    pub fn ring(&mut self) -> &mut [i16] {
        unsafe { core::slice::from_raw_parts_mut(phys::phys_to_virt(self.ring) as *mut i16, RING_FRAMES * 2) }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The frame DAC2 plays next: its current count, in longwords.
    pub fn position(&self) -> usize {
        unsafe {
            outl(self.io + MEM_PAGE, PAGE_DAC);
            (inl(self.io + DAC2_SIZE) >> 16) as usize % RING_FRAMES
        }
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// Start playing the ring from frame 0 (the caller has filled it).
    pub fn start(&mut self) {
        let io = self.io;
        unsafe {
            outl(io + CONTROL, self.ctrl);
            outl(io + MEM_PAGE, PAGE_DAC);
            outl(io + DAC2_FRAME, self.ring as u32);
            // Size in longwords, less one; the current count from 0.
            outl(io + DAC2_SIZE, RING_BYTES as u32 / 4 - 1);
            outl(io + SERIAL, P2_MODE_16_STEREO | P2_END_INC);
            // Sample frames between (unused) interrupts, less one.
            outl(io + DAC2_COUNT, RING_FRAMES as u32 / 4 - 1);
            outl(io + CONTROL, self.ctrl | DAC2_EN);
        }
        self.running = true;
    }

    pub fn stop(&mut self) {
        unsafe {
            outl(self.io + SERIAL, P2_PAUSE);
            outl(self.io + CONTROL, self.ctrl);
        }
        self.running = false;
    }

    pub fn diag(&self) -> alloc::string::String {
        let io = self.io;
        unsafe {
            outl(io + MEM_PAGE, PAGE_DAC);
            alloc::format!(
                "control {:#x}, status {:#x}, serial {:#x}, DAC2 frame {:#x} size {:#x} count {:#x}, src {:#x}",
                inl(io + CONTROL),
                inl(io + STATUS),
                inl(io + SERIAL),
                inl(io + DAC2_FRAME),
                inl(io + DAC2_SIZE),
                inl(io + DAC2_COUNT),
                inl(io + SRC_1371)
            )
        }
    }
}

/// DACs, ADC and interrupts off.
fn quiet(io: u16, ctrl: u32) {
    unsafe {
        outl(io + SERIAL, 0);
        outl(io + CONTROL, ctrl & !(0x7 << 4));
    }
}

fn spin(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..SPINS {
        if cond() {
            return true;
        }
    }
    false
}

/// The ES1371's sample rate converter and AC'97 codec.
struct Es1371 {
    io: u16,
    /// [`CODEC_EV1938`] on the EV1938, else 0.
    magic: u32,
}

impl Es1371 {
    fn src_ready(&self) -> u32 {
        let io = self.io;
        let mut v = 0;
        spin(|| {
            v = unsafe { inl(io + SRC_1371) };
            v & SRC_BUSY == 0
        });
        v
    }

    fn src_write(&self, reg: u32, v: u32) {
        let r = self.src_ready() & SRC_KEEP;
        unsafe { outl(self.io + SRC_1371, r | (reg & 0x7f) << 25 | (v & 0xffff) | SRC_WE) };
    }

    /// Wait for the converter's state machine to be between accesses
    /// (state 1) so that a codec access does not collide with it; the
    /// returned value restores the converter afterwards.
    fn codec_window(&self) -> u32 {
        let io = self.io;
        let saved = self.src_ready();
        unsafe { outl(io + SRC_1371, (saved & SRC_KEEP) | 0x0001_0000) };
        spin(|| unsafe { inl(io + SRC_1371) } & 0x0087_0000 == 0);
        spin(|| unsafe { inl(io + SRC_1371) } & 0x0087_0000 == 0x0001_0000);
        saved
    }

    fn codec_write(&self, reg: u32, v: u32) {
        let io = self.io;
        spin(|| unsafe { inl(io + CODEC_1371) } & CODEC_WIP == 0);
        let saved = self.codec_window();
        unsafe { outl(io + CODEC_1371, (reg & 0x7f) << 16 | (v & 0xffff) | self.magic) };
        self.src_ready();
        unsafe { outl(io + SRC_1371, saved) };
    }

    fn codec_read(&self, reg: u32) -> Option<u16> {
        let io = self.io;
        for _ in 0..4 {
            spin(|| unsafe { inl(io + CODEC_1371) } & CODEC_WIP == 0);
            let saved = self.codec_window();
            unsafe { outl(io + CODEC_1371, (reg & 0x7f) << 16 | CODEC_PIRD | self.magic) };
            self.src_ready();
            unsafe { outl(io + SRC_1371, saved) };
            spin(|| unsafe { inl(io + CODEC_1371) } & CODEC_WIP == 0);
            let mut v = 0;
            if spin(|| {
                v = unsafe { inl(io + CODEC_1371) };
                v & CODEC_RDY != 0
            }) {
                if self.magic != 0 {
                    // The EV1938 shows the data a little after RDY.
                    for _ in 0..100 {
                        unsafe { inl(io + CONTROL) };
                    }
                    v = unsafe { inl(io + CODEC_1371) };
                }
                return Some(v as u16);
            }
        }
        None
    }

    /// Clear the converter's RAM and set unity volumes, then DAC2's rate.
    /// Every channel is set up for 48000 Hz (N = 16, integer step 16),
    /// the ADC too though it stays unused: the converter serves the
    /// channels in turn.
    fn src_init(&self, rate: u32) {
        let io = self.io;
        self.src_ready();
        unsafe { outl(io + SRC_1371, SRC_DISABLE) };
        for r in 0..0x80 {
            self.src_write(r, 0);
        }
        for base in [SR_DAC1, SR_DAC2, SR_ADC] {
            self.src_write(base + SR_TRUNC_N, 16 << 4);
            self.src_write(base + SR_INT_REGS, 16 << 10);
        }
        for v in [SR_VOL_ADC, SR_VOL_DAC1, SR_VOL_DAC2] {
            self.src_write(v, 1 << 12);
            self.src_write(v + 1, 1 << 12);
        }
        // DAC2: the rate in 1/32768ths of 3000 Hz, integer part in the
        // top six bits of INT_REGS, fraction in VFREQ_FRAC; the channel
        // held while it changes.
        let freq = ((rate << 15) + 1500) / 3000;
        let r = self.src_ready() & (SRC_DISABLE | SRC_DIS_P1 | SRC_DIS_R1);
        unsafe { outl(io + SRC_1371, r | SRC_DIS_P2) };
        self.src_write(SR_DAC2 + SR_INT_REGS, (freq >> 5) & 0xfc00);
        self.src_write(SR_DAC2 + SR_VFREQ_FRAC, freq & 0x7fff);
        self.src_ready();
        unsafe { outl(io + SRC_1371, 0) };
    }
}

/// Bring up an AudioPCI chip: DAC2 at its rate, the codec unmuted.
pub fn probe(dev: &pci::Device) -> Option<Output> {
    dev.enable();
    dev.set_intx_disabled(true);
    let chip = if (dev.vendor, dev.device) == (0x1274, 0x5000) { Chip::Es1370 } else { Chip::Es1371 };
    let rev = dev.read(0x08) & 0xff;
    let label = if chip == Chip::Es1370 { "ES1370" } else if dev.device == 0x5880 { "CT5880" } else { "ES1371" };
    let name = alloc::format!("es137x {}: {:04x}:{:04x} {label} rev {rev}", dev.bdf(), dev.vendor, dev.device);
    let b0 = dev.read(0x10);
    if b0 & 1 == 0 || b0 & !3 == 0 {
        crate::println!("{name}: no I/O BAR");
        return None;
    }
    let io = (b0 & 0xffc0) as u16;
    let (ctrl, rate, codec) = match chip {
        Chip::Es1370 => {
            let rate = 44100;
            let ctrl = CDC_EN_1370 | SERR_DIS_1370 | pclkdiv_1370(rate);
            quiet(io, ctrl);
            let w = |r: u16, v: u16| {
                spin(|| unsafe { inl(io + STATUS) } & CSTAT_1370 == 0);
                unsafe { outw(io + CODEC_1370, r << 8 | v) };
            };
            // AK4531: out of reset, clocks from the chip; master 0 dB,
            // voice (DAC2) 0 dB, both unmuted; voice and the usual
            // sources to the output mixer.
            w(0x16, 0x03);
            w(0x17, 0x00);
            for (r, v) in [(0x00, 0x00), (0x01, 0x00), (0x02, 0x06), (0x03, 0x06), (0x10, 0x1f), (0x11, 0x0f)] {
                w(r, v);
            }
            (ctrl, rate, alloc::string::String::from("AK4531"))
        }
        Chip::Es1371 => {
            let ctrl = 0;
            quiet(io, ctrl);
            let c = Es1371 { io, magic: if dev.device == 0x8938 { CODEC_EV1938 } else { 0 } };
            unsafe {
                outl(io + LEGACY_1371, 0);
            }
            // Revisions whose codec needs the AC'97 reset line released:
            // CT5880 C, D and E, ES1371 rev 7 and 8 (CT5880 A).
            if matches!((dev.device, rev), (0x5880, 2..=4) | (0x1371, 7 | 8)) {
                unsafe { outl(io + STATUS, AC97_RST_1371) };
                msi::poll(20, || {}, || false);
            }
            unsafe {
                outl(io + CONTROL, ctrl | SYNC_RES_1371);
                inl(io + CONTROL);
            }
            msi::poll(1, || {}, || false);
            unsafe { outl(io + CONTROL, ctrl) };
            let rate = 48000;
            c.src_init(rate);
            c.codec_write(AC_RESET, 0);
            let ready = msi::poll(600, || {}, || c.codec_read(AC_POWERDOWN).is_some_and(|v| v & 0xe == 0xe));
            let id = c.codec_read(AC_VENDOR1).zip(c.codec_read(AC_VENDOR2));
            let Some((v1, v2)) = id.filter(|&(a, b)| ready || (a, b) != (0xffff, 0xffff)) else {
                crate::println!("{name}: no codec");
                quiet(io, ctrl);
                return None;
            };
            c.codec_write(AC_MASTER, 0x0000);
            c.codec_write(AC_HEADPHONE, 0x0000);
            c.codec_write(AC_PCM_OUT, 0x0808);
            (ctrl, rate, alloc::format!("AC'97 codec {v1:04x}:{v2:04x}{}", if ready { "" } else { " (not ready)" }))
        }
    };
    // DMA memory below 4 GB (the frame address register is 32 bits).
    let Some(ring) = phys::alloc_contig((RING_BYTES / 4096) as u64, 0xffff_ffff) else {
        quiet(io, ctrl);
        return None;
    };
    crate::println!("{name}: {codec}, DAC2 {rate} Hz, 16-bit stereo");
    Some(Output { io, ctrl, ring, rate, running: false })
}
