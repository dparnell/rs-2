//! Intel ICH AC'97 audio controllers (82801AA and later, AMD and nVidia
//! chipsets with the same interface): QEMU's `AC97`, VirtualBox's
//! default sound card, Proxmox's `ac97`, older VMware.
//!
//! Output only. The native audio mixer registers (NAMBAR, I/O BAR 0)
//! reach the primary codec; the bus master registers (NABMBAR, I/O BAR
//! 1) hold the PCM out DMA engine. The link is cold reset, the codec
//! waits for its DAC to be ready, master and PCM out volumes are set to
//! 0 dB unmuted and the front DAC runs at 48 kHz (set through variable
//! rate audio when the codec has it, else its fixed rate). PCM out plays
//! a ring of [`RING_FRAMES`] 16-bit stereo frames described by all 32
//! buffer descriptors; the last valid index is kept one behind the
//! current one, so the engine never runs dry. As with HD Audio, the
//! mixer fills the ring from the timer tick: no interrupts.

use core::ptr::write_volatile;
use core::sync::atomic::{fence, Ordering};

use super::pci;
use crate::arch::{inb, inl, inw, outb, outl, outw};
use crate::audio::RING_FRAMES;
use crate::mm::phys;
use crate::msi;

const RING_BYTES: usize = RING_FRAMES * 4;
/// Buffer descriptors (the hardware's fixed count).
const BDL_ENTRIES: usize = 32;
/// Frames per descriptor.
const ENTRY_FRAMES: usize = RING_FRAMES / BDL_ENTRIES;

// Mixer registers (NAMBAR).
const RESET: u16 = 0x00;
const MASTER: u16 = 0x02;
const HEADPHONE: u16 = 0x04;
const PCM_OUT: u16 = 0x18;
const POWERDOWN: u16 = 0x26;
const EXT_ID: u16 = 0x28;
const EXT_CTRL: u16 = 0x2a;
const FRONT_RATE: u16 = 0x2c;
const VENDOR1: u16 = 0x7c;
const VENDOR2: u16 = 0x7e;

// Bus master registers (NABMBAR): PCM out box and global registers.
const PO_BDBAR: u16 = 0x10;
const PO_CIV: u16 = 0x14;
const PO_LVI: u16 = 0x15;
const PO_SR: u16 = 0x16;
const PO_PICB: u16 = 0x18;
const PO_CR: u16 = 0x1b;
const PI_CR: u16 = 0x0b;
const MC_CR: u16 = 0x2b;
const GLOB_CNT: u16 = 0x2c;
const GLOB_STA: u16 = 0x30;
const CAS: u16 = 0x34;

const CR_RPBM: u8 = 1;
const CR_RR: u8 = 2;
/// GLOB_CNT: AC'97 cold reset released (0 holds the link in reset).
const CNT_COLD: u32 = 2;
/// GLOB_STA: primary codec ready.
const STA_PCR: u32 = 1 << 8;

/// Controllers with Intel's ICH register layout.
pub fn is_ac97(dev: &pci::Device) -> bool {
    matches!(
        (dev.vendor, dev.device),
        // Intel 82801AA/AB (ICH, ICH0), ICH2, ICH3, ICH4, ICH5, ESB,
        // ICH6, ICH7, 440MX.
        (0x8086, 0x2415 | 0x2425 | 0x2445 | 0x2485 | 0x24c5 | 0x24d5 | 0x25a6 | 0x266e | 0x27de | 0x7195)
            // AMD 768 and 8111.
            | (0x1022, 0x7445 | 0x746d)
            // nVidia nForce, nForce2, nForce3, CK8, CK8S, CK804, MCP04.
            | (0x10de, 0x01b1 | 0x006a | 0x00da | 0x008a | 0x00ea | 0x0059 | 0x003a)
    )
}

/// The PCM out engine: the ring the mixer fills.
pub struct Output {
    bm: u16,
    bdl: u64,
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

    /// The frame the hardware plays next: the current descriptor and the
    /// samples left in it. Also moves the last valid index along.
    pub fn position(&self) -> usize {
        let bm = self.bm;
        let (civ, picb) = loop {
            let civ = unsafe { inb(bm + PO_CIV) } as usize & 31;
            let picb = unsafe { inw(bm + PO_PICB) } as usize;
            if unsafe { inb(bm + PO_CIV) } as usize & 31 == civ {
                break (civ, picb);
            }
        };
        if self.running {
            unsafe { outb(bm + PO_LVI, ((civ + BDL_ENTRIES - 1) % BDL_ENTRIES) as u8) };
        }
        let left = (picb / 2).min(ENTRY_FRAMES);
        (civ * ENTRY_FRAMES + ENTRY_FRAMES - left) % RING_FRAMES
    }

    pub fn running(&self) -> bool {
        self.running
    }

    fn reset_engine(&self) {
        let bm = self.bm;
        unsafe {
            outb(bm + PO_CR, 0);
            outb(bm + PO_CR, CR_RR);
        }
        msi::poll(10, || {}, || unsafe { inb(bm + PO_CR) } & CR_RR == 0);
        unsafe { outw(bm + PO_SR, 0x1c) };
    }

    /// Start playing the ring from frame 0 (the caller has filled it).
    pub fn start(&mut self) {
        self.reset_engine();
        let bm = self.bm;
        unsafe {
            outl(bm + PO_BDBAR, self.bdl as u32);
            outb(bm + PO_LVI, BDL_ENTRIES as u8 - 1);
            fence(Ordering::SeqCst);
            // No interrupts (IOCE, FEIE, LVBIE clear).
            outb(bm + PO_CR, CR_RPBM);
        }
        self.running = true;
    }

    pub fn stop(&mut self) {
        unsafe { outb(self.bm + PO_CR, 0) };
        self.running = false;
    }

    pub fn diag(&self) -> alloc::string::String {
        let bm = self.bm;
        unsafe {
            alloc::format!(
                "PCM out civ {} lvi {} sr {:#x} picb {} cr {:#x}, glob_cnt {:#x} glob_sta {:#x}",
                inb(bm + PO_CIV),
                inb(bm + PO_LVI),
                inw(bm + PO_SR),
                inw(bm + PO_PICB),
                inb(bm + PO_CR),
                inl(bm + GLOB_CNT),
                inl(bm + GLOB_STA)
            )
        }
    }
}

/// Quiet the controller: every DMA engine stopped and reset, interrupts
/// off.
fn quiet(bm: u16) {
    for cr in [PI_CR, PO_CR, MC_CR] {
        unsafe {
            outb(bm + cr, 0);
            outb(bm + cr, CR_RR);
        }
    }
    let cnt = unsafe { inl(bm + GLOB_CNT) };
    unsafe { outl(bm + GLOB_CNT, cnt & !1) };
}

struct Codec {
    mix: u16,
    bm: u16,
}

impl Codec {
    /// Wait for the codec access semaphore (reads 0 when it is ours).
    fn claim(&self) {
        let bm = self.bm;
        msi::poll(5, || {}, || unsafe { inb(bm + CAS) } & 1 == 0);
    }

    fn read(&self, r: u16) -> u16 {
        self.claim();
        unsafe { inw(self.mix + r) }
    }

    fn write(&self, r: u16, v: u16) {
        self.claim();
        unsafe { outw(self.mix + r, v) };
    }
}

/// Bring up an AC'97 controller and its primary codec.
pub fn probe(dev: &pci::Device) -> Option<Output> {
    dev.enable();
    dev.set_intx_disabled(true);
    let (b0, b1) = (dev.read(0x10), dev.read(0x14));
    let name = alloc::format!("ac97 {}: {:04x}:{:04x}", dev.bdf(), dev.vendor, dev.device);
    if b0 & 1 == 0 || b1 & 1 == 0 || b0 & !3 == 0 || b1 & !3 == 0 {
        crate::println!("{name}: no I/O BARs");
        return None;
    }
    // ICH4 and later: the I/O BARs answer only with IOSE set.
    if dev.vendor == 0x8086 && matches!(dev.device, 0x24c5 | 0x24d5 | 0x25a6 | 0x266e | 0x27de) {
        let v = dev.read(0x40);
        dev.write(0x40, v | 1 << 8);
    }
    let (mix, bm) = ((b0 & 0xfffc) as u16, (b1 & 0xffc0) as u16);
    quiet(bm);
    // Cold reset: hold the link in reset, then release it and wait for
    // the primary codec.
    unsafe { outl(bm + GLOB_CNT, 0) };
    msi::poll(1, || {}, || false);
    unsafe { outl(bm + GLOB_CNT, CNT_COLD) };
    if !msi::poll(600, || {}, || unsafe { inl(bm + GLOB_STA) } & STA_PCR != 0) {
        crate::println!("{name}: no codec");
        quiet(bm);
        return None;
    }
    let c = Codec { mix, bm };
    c.write(RESET, 0);
    // DAC, analog mixer and reference ready.
    if !msi::poll(600, || {}, || c.read(POWERDOWN) & 0xe == 0xe) {
        crate::println!("{name}: codec not ready ({:#06x})", c.read(POWERDOWN));
    }
    let id = (c.read(VENDOR1) as u32) << 16 | c.read(VENDOR2) as u32;
    // 0 dB, unmuted.
    c.write(MASTER, 0x0000);
    c.write(HEADPHONE, 0x0000);
    c.write(PCM_OUT, 0x0808);
    let ext = c.read(EXT_ID);
    let mut rate = 48000;
    let vra = ext != 0xffff && ext & 1 != 0;
    if vra {
        c.write(EXT_CTRL, c.read(EXT_CTRL) | 1);
        c.write(FRONT_RATE, 48000);
        let r = c.read(FRONT_RATE) as u32;
        if (8000..=48000).contains(&r) {
            rate = r;
        }
    }
    // 16-bit stereo PCM: the channel and sample size fields of GLOB_CNT
    // left at 0.
    let cnt = unsafe { inl(bm + GLOB_CNT) } & !(3 << 20 | 3 << 22);
    unsafe { outl(bm + GLOB_CNT, cnt | CNT_COLD) };
    // DMA memory below 4 GB: the descriptor list, then the ring.
    let Some(mem) = phys::alloc_contig(1 + (RING_BYTES / 4096) as u64, 0xffff_ffff) else {
        quiet(bm);
        return None;
    };
    let (bdl, ring) = (mem, mem + 4096);
    let b = phys::phys_to_virt(bdl) as *mut u32;
    for i in 0..BDL_ENTRIES {
        let a = ring + (i * ENTRY_FRAMES * 4) as u64;
        unsafe {
            write_volatile(b.add(i * 2), a as u32);
            // Samples (16-bit) in the buffer; no interrupt on completion.
            write_volatile(b.add(i * 2 + 1), (ENTRY_FRAMES * 2) as u32);
        }
    }
    crate::println!("{name}: codec {:04x}:{:04x}{}, {} Hz, 16-bit stereo", id >> 16, id & 0xffff, if vra { ", variable rate" } else { "" }, rate);
    let o = Output { bm, bdl, ring, rate, running: false };
    o.reset_engine();
    Some(o)
}
