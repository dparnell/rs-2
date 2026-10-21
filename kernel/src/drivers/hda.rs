//! Intel High Definition Audio controllers (PCI class 04.03): ICH6 and
//! later chipsets, QEMU's `intel-hda`/`ich9-intel-hda`, VMware's "HD
//! Audio" and Proxmox's ich9-intel-hda.
//!
//! Output only. The controller is reset, codec verbs go through the CORB
//! and RIRB rings (or the immediate command registers when the rings do
//! not answer), and each codec's audio function group is walked for an
//! output pin (line out, speaker, headphone) and the path from it back to
//! a DAC through mixers and selectors. Every such pin is enabled, the
//! amplifiers on its path are unmuted at 0 dB and the DACs are bound to
//! one output stream: 48 kHz, 16-bit stereo, played from a ring of
//! [`RING_FRAMES`] frames described by a buffer descriptor list of four
//! entries. The software mixer (`crate::audio`) fills the ring ahead of
//! the hardware's link position (LPIB) from the timer tick, so the stream
//! raises no interrupts.

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::pci;
use crate::mm::{mmio, phys};
use crate::msi;

const PAGE: usize = 4096;

use crate::audio::RING_FRAMES;

const RING_BYTES: usize = RING_FRAMES * 4;
/// Buffer descriptor list entries the ring is split into.
const BDL_ENTRIES: usize = 4;
/// Stream tag of the output stream (1-15).
const TAG: u32 = 1;
/// 48 kHz, 16 bits, 2 channels (stream format and converter format).
const FORMAT: u16 = 0x0011;

// Global registers.
const GCAP: usize = 0x00;
const VMIN: usize = 0x02;
const VMAJ: usize = 0x03;
const GCTL: usize = 0x08;
const STATESTS: usize = 0x0e;
const INTCTL: usize = 0x20;
const CORBLBASE: usize = 0x40;
const CORBUBASE: usize = 0x44;
const CORBWP: usize = 0x48;
const CORBRP: usize = 0x4a;
const CORBCTL: usize = 0x4c;
const CORBSIZE: usize = 0x4e;
const RIRBLBASE: usize = 0x50;
const RIRBUBASE: usize = 0x54;
const RIRBWP: usize = 0x58;
const RINTCNT: usize = 0x5a;
const RIRBCTL: usize = 0x5c;
const RIRBSTS: usize = 0x5d;
const RIRBSIZE: usize = 0x5e;
const ICOI: usize = 0x60;
const ICII: usize = 0x64;
const ICIS: usize = 0x68;
/// First stream descriptor; input streams come first, then output.
const SD_BASE: usize = 0x80;

// Stream descriptor registers (relative).
const SD_CTL: usize = 0x00;
const SD_STS: usize = 0x03;
const SD_LPIB: usize = 0x04;
const SD_CBL: usize = 0x08;
const SD_LVI: usize = 0x0c;
const SD_FMT: usize = 0x12;
const SD_BDPL: usize = 0x18;
const SD_BDPU: usize = 0x1c;

const SD_CTL_SRST: u32 = 1;
const SD_CTL_RUN: u32 = 2;

// Verbs (12-bit verb, 8-bit payload).
const GET_PARAM: u32 = 0xf00;
const GET_CONN_LIST: u32 = 0xf02;
const SET_CONN_SELECT: u32 = 0x701;
const SET_POWER: u32 = 0x705;
const SET_STREAM: u32 = 0x706;
const SET_PIN_CTL: u32 = 0x707;
const SET_EAPD: u32 = 0x70c;
const GET_CONFIG_DEFAULT: u32 = 0xf1c;
// Verbs with a 16-bit payload (4-bit verb).
const SET_FORMAT: u32 = 0x2;
const SET_AMP: u32 = 0x3;

// Parameters.
const P_VENDOR: u32 = 0x00;
const P_NODES: u32 = 0x04;
const P_FG_TYPE: u32 = 0x05;
const P_WIDGET_CAPS: u32 = 0x09;
const P_PIN_CAPS: u32 = 0x0c;
const P_IN_AMP: u32 = 0x0d;
const P_CONN_LEN: u32 = 0x0e;
const P_OUT_AMP: u32 = 0x12;

// Widget types (capabilities bits 23:20).
const W_OUTPUT: u32 = 0;
const W_MIXER: u32 = 2;
const W_SELECTOR: u32 = 3;
const W_PIN: u32 = 4;

const CMD_TIMEOUT_MS: u64 = 50;

fn rd8(r: *mut u8, o: usize) -> u8 {
    unsafe { read_volatile(r.add(o)) }
}
fn rd16(r: *mut u8, o: usize) -> u16 {
    unsafe { read_volatile(r.add(o) as *const u16) }
}
fn rd32(r: *mut u8, o: usize) -> u32 {
    unsafe { read_volatile(r.add(o) as *const u32) }
}
fn wr8(r: *mut u8, o: usize, v: u8) {
    unsafe { write_volatile(r.add(o), v) }
}
fn wr16(r: *mut u8, o: usize, v: u16) {
    unsafe { write_volatile(r.add(o) as *mut u16, v) }
}
fn wr32(r: *mut u8, o: usize, v: u32) {
    unsafe { write_volatile(r.add(o) as *mut u32, v) }
}

fn wait(r: *mut u8, ms: u64, cond: impl FnMut() -> bool) -> bool {
    msi::poll(ms, || { let _ = rd32(r, GCTL); }, cond)
}

/// How codec verbs reach the codecs.
struct Link {
    regs: *mut u8,
    /// CORB and RIRB (virtual addresses) and their entry counts; `None`
    /// once the immediate command registers are used instead.
    rings: Option<(*mut u32, *mut u32, usize, usize)>,
    rirb_rp: usize,
}

impl Link {
    /// Send `verb` to node `nid` of codec `cad`; the response, if any.
    fn cmd(&mut self, cad: u32, nid: u32, verb: u32) -> Option<u32> {
        let v = cad << 28 | (nid & 0x7f) << 20 | (verb & 0xfffff);
        if let Some(r) = self.ring_cmd(v) {
            return Some(r);
        }
        self.immediate(v)
    }

    fn ring_cmd(&mut self, v: u32) -> Option<u32> {
        let (corb, rirb, nc, nr) = self.rings?;
        let regs = self.regs;
        let wp = (rd16(regs, CORBWP) as usize & 0xff) + 1;
        let wp = wp % nc;
        unsafe { write_volatile(corb.add(wp), v) };
        fence(Ordering::SeqCst);
        wr16(regs, CORBWP, wp as u16);
        loop {
            let rp = self.rirb_rp;
            let ok = wait(regs, CMD_TIMEOUT_MS, || rd16(regs, RIRBWP) as usize & 0xff != rp);
            if !ok {
                crate::println!("hda: CORB/RIRB do not answer; using immediate commands");
                self.rings = None;
                wr8(regs, CORBCTL, 0);
                wr8(regs, RIRBCTL, 0);
                return None;
            }
            self.rirb_rp = (rp + 1) % nr;
            // Acknowledge: the controller holds further commands after
            // RINTCNT responses until the flag is cleared.
            wr8(regs, RIRBSTS, 5);
            fence(Ordering::SeqCst);
            let (resp, ex) = unsafe {
                (read_volatile(rirb.add(self.rirb_rp * 2)), read_volatile(rirb.add(self.rirb_rp * 2 + 1)))
            };
            // Unsolicited responses (jack events) are not answers.
            if ex & 0x10 == 0 {
                return Some(resp);
            }
        }
    }

    fn immediate(&mut self, v: u32) -> Option<u32> {
        let regs = self.regs;
        if !wait(regs, CMD_TIMEOUT_MS, || rd16(regs, ICIS) & 1 == 0) {
            return None;
        }
        // Clear "result valid", then send.
        wr16(regs, ICIS, 2);
        wr32(regs, ICOI, v);
        wr16(regs, ICIS, 1);
        if !wait(regs, CMD_TIMEOUT_MS, || rd16(regs, ICIS) & 3 == 2) {
            return None;
        }
        let r = rd32(regs, ICII);
        wr16(regs, ICIS, 2);
        Some(r)
    }
}

/// A widget of an audio function group.
struct Widget {
    nid: u32,
    caps: u32,
    conns: Vec<u32>,
    pin_caps: u32,
    config: u32,
}

impl Widget {
    fn kind(&self) -> u32 {
        self.caps >> 20 & 0xf
    }
}

/// An audio function group and its widgets.
struct Group {
    cad: u32,
    afg: u32,
    widgets: Vec<Widget>,
    /// Default amplifier capabilities (for widgets without their own).
    out_amp: u32,
    in_amp: u32,
}

impl Group {
    fn widget(&self, nid: u32) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.nid == nid)
    }

    /// The path from `nid` to a DAC: nodes from the pin to the DAC, each
    /// with the connection index taken to the next.
    fn path(&self, nid: u32, depth: usize, out: &mut Vec<(u32, usize)>) -> bool {
        let Some(w) = self.widget(nid) else { return false };
        if w.kind() == W_OUTPUT {
            out.push((nid, 0));
            return true;
        }
        if depth == 0 || !matches!(w.kind(), W_PIN | W_MIXER | W_SELECTOR) {
            return false;
        }
        for (i, &c) in w.conns.iter().enumerate() {
            out.push((nid, i));
            if self.path(c, depth - 1, out) {
                return true;
            }
            out.pop();
        }
        false
    }
}

fn pin_name(config: u32) -> &'static str {
    match config >> 20 & 0xf {
        0 => "line out",
        1 => "speaker",
        2 => "headphone",
        _ => "output",
    }
}

/// Read a codec's widgets: the first audio function group.
fn read_group(l: &mut Link, cad: u32) -> Option<Group> {
    let nodes = l.cmd(cad, 0, GET_PARAM << 8 | P_NODES)?;
    let (start, count) = (nodes >> 16 & 0xff, nodes & 0xff);
    let afg = (start..start + count).find(|&n| l.cmd(cad, n, GET_PARAM << 8 | P_FG_TYPE).is_some_and(|t| t & 0xff == 1))?;
    l.cmd(cad, afg, SET_POWER << 8);
    let out_amp = l.cmd(cad, afg, GET_PARAM << 8 | P_OUT_AMP).unwrap_or(0);
    let in_amp = l.cmd(cad, afg, GET_PARAM << 8 | P_IN_AMP).unwrap_or(0);
    let sub = l.cmd(cad, afg, GET_PARAM << 8 | P_NODES)?;
    let (start, count) = (sub >> 16 & 0xff, sub & 0xff);
    let mut widgets = Vec::new();
    for nid in start..start + count {
        let caps = l.cmd(cad, nid, GET_PARAM << 8 | P_WIDGET_CAPS).unwrap_or(0);
        let mut conns = Vec::new();
        if caps & 1 << 8 != 0 {
            let len = l.cmd(cad, nid, GET_PARAM << 8 | P_CONN_LEN).unwrap_or(0);
            let long = len & 0x80 != 0;
            let n = (len & 0x7f) as usize;
            let per = if long { 2 } else { 4 };
            let mut prev = 0;
            let mut i = 0;
            while i < n.min(32) {
                let e = l.cmd(cad, nid, GET_CONN_LIST << 8 | i as u32).unwrap_or(0);
                for k in 0..per {
                    if i + k >= n {
                        break;
                    }
                    let (v, range) = if long {
                        (e >> (16 * k) & 0x7fff, e >> (16 * k) & 0x8000 != 0)
                    } else {
                        (e >> (8 * k) & 0x7f, e >> (8 * k) & 0x80 != 0)
                    };
                    // A range entry lists every node from the previous one.
                    if range && prev != 0 && v > prev {
                        conns.extend(prev + 1..=v.min(prev + 32));
                    } else {
                        conns.push(v);
                    }
                    prev = v;
                }
                i += per;
            }
        }
        let kind = caps >> 20 & 0xf;
        let (pin_caps, config) = if kind == W_PIN {
            (
                l.cmd(cad, nid, GET_PARAM << 8 | P_PIN_CAPS).unwrap_or(0),
                l.cmd(cad, nid, GET_CONFIG_DEFAULT << 8).unwrap_or(0),
            )
        } else {
            (0, 0)
        };
        widgets.push(Widget { nid, caps, conns, pin_caps, config });
    }
    Some(Group { cad, afg, widgets, out_amp, in_amp })
}

/// Set an amplifier to 0 dB (or its top, if lower), unmuted.
fn unmute(l: &mut Link, g: &Group, w: &Widget, output: bool, index: usize) {
    let present = if output { w.caps & 1 << 2 } else { w.caps & 1 << 1 };
    if present == 0 {
        return;
    }
    let caps = if w.caps & 1 << 3 != 0 {
        l.cmd(g.cad, w.nid, GET_PARAM << 8 | if output { P_OUT_AMP } else { P_IN_AMP }).unwrap_or(0)
    } else if output {
        g.out_amp
    } else {
        g.in_amp
    };
    let gain = (caps & 0x7f).min(caps >> 8 & 0x7f);
    let dir = if output { 1 << 15 } else { 1 << 14 };
    let payload = dir | 3 << 12 | (index as u32 & 0xf) << 8 | gain;
    l.cmd(g.cad, w.nid, SET_AMP << 16 | payload);
}

/// Find the output pins of a codec and route them to its DACs. Returns
/// a description of the paths.
fn route(l: &mut Link, g: &Group) -> Vec<String> {
    let mut pins: Vec<&Widget> = g
        .widgets
        .iter()
        .filter(|w| {
            w.kind() == W_PIN
                && w.pin_caps & 1 << 4 != 0
                // Port connectivity 1: nothing attached.
                && w.config >> 30 != 1
                && matches!(w.config >> 20 & 0xf, 0..=2)
        })
        .collect();
    // Line out first, then speakers, then headphones.
    pins.sort_by_key(|w| w.config >> 20 & 0xf);
    let mut out = Vec::new();
    for pin in pins {
        let mut path = Vec::new();
        if !g.path(pin.nid, 6, &mut path) {
            continue;
        }
        for &(nid, idx) in &path {
            let Some(w) = g.widget(nid) else { continue };
            // Power state control.
            if w.caps & 1 << 10 != 0 {
                l.cmd(g.cad, nid, SET_POWER << 8);
            }
            match w.kind() {
                W_OUTPUT => {
                    l.cmd(g.cad, nid, SET_FORMAT << 16 | FORMAT as u32);
                    l.cmd(g.cad, nid, SET_STREAM << 8 | TAG << 4);
                    unmute(l, g, w, true, 0);
                }
                W_PIN => {
                    if w.conns.len() > 1 {
                        l.cmd(g.cad, nid, SET_CONN_SELECT << 8 | idx as u32);
                    }
                    let hp = if w.config >> 20 & 0xf == 2 { 0x80 } else { 0 };
                    l.cmd(g.cad, nid, SET_PIN_CTL << 8 | 0x40 | hp);
                    if w.pin_caps & 1 << 16 != 0 {
                        l.cmd(g.cad, nid, SET_EAPD << 8 | 2);
                    }
                    unmute(l, g, w, true, 0);
                }
                W_SELECTOR => {
                    l.cmd(g.cad, nid, SET_CONN_SELECT << 8 | idx as u32);
                    unmute(l, g, w, true, 0);
                }
                W_MIXER => {
                    unmute(l, g, w, false, idx);
                    unmute(l, g, w, true, 0);
                }
                _ => {}
            }
        }
        let chain: Vec<String> = path.iter().map(|&(n, _)| alloc::format!("{n:#x}")).collect();
        out.push(alloc::format!("{} {}", pin_name(pin.config), chain.join(" <- ")));
    }
    out
}

/// The output stream: the ring the mixer fills.
pub struct Output {
    regs: *mut u8,
    sd: usize,
    bdl: u64,
    ring: u64,
    running: bool,
}

unsafe impl Send for Output {}

impl Output {
    /// The ring of interleaved stereo samples.
    pub fn ring(&mut self) -> &mut [i16] {
        unsafe { core::slice::from_raw_parts_mut(phys::phys_to_virt(self.ring) as *mut i16, RING_FRAMES * 2) }
    }

    /// The frame the hardware plays next (from the link position).
    pub fn position(&self) -> usize {
        (rd32(self.regs, self.sd + SD_LPIB) as usize % RING_BYTES) / 4
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn rate(&self) -> u32 {
        48000
    }

    fn reset_stream(&self) {
        let (r, sd) = (self.regs, self.sd);
        let ctl = rd32(r, sd + SD_CTL);
        wr32(r, sd + SD_CTL, ctl & !SD_CTL_RUN);
        wait(r, 10, || rd32(r, sd + SD_CTL) & SD_CTL_RUN == 0);
        wr32(r, sd + SD_CTL, (rd32(r, sd + SD_CTL) & 0xff_ffff) | SD_CTL_SRST);
        wait(r, 10, || rd32(r, sd + SD_CTL) & SD_CTL_SRST != 0);
        wr32(r, sd + SD_CTL, rd32(r, sd + SD_CTL) & 0xff_ffff & !SD_CTL_SRST);
        wait(r, 10, || rd32(r, sd + SD_CTL) & SD_CTL_SRST == 0);
        // Clear the status bits (write 1 to clear).
        wr8(r, sd + SD_STS, 0x1c);
    }

    /// Start playing the ring from frame 0 (the caller has filled it).
    pub fn start(&mut self) {
        self.reset_stream();
        let (r, sd) = (self.regs, self.sd);
        wr32(r, sd + SD_BDPL, self.bdl as u32);
        wr32(r, sd + SD_BDPU, (self.bdl >> 32) as u32);
        wr32(r, sd + SD_CBL, RING_BYTES as u32);
        wr16(r, sd + SD_LVI, BDL_ENTRIES as u16 - 1);
        wr16(r, sd + SD_FMT, FORMAT);
        // Stream number in bits 23:20, no interrupts.
        let ctl = rd32(r, sd + SD_CTL) & 0xff_ffff & !(0xf << 20 | 0x1c);
        wr32(r, sd + SD_CTL, ctl | TAG << 20);
        fence(Ordering::SeqCst);
        wr32(r, sd + SD_CTL, rd32(r, sd + SD_CTL) & 0xff_ffff | SD_CTL_RUN);
        self.running = true;
    }

    pub fn stop(&mut self) {
        let (r, sd) = (self.regs, self.sd);
        wr32(r, sd + SD_CTL, rd32(r, sd + SD_CTL) & 0xff_ffff & !SD_CTL_RUN);
        self.running = false;
    }

    pub fn diag(&self) -> String {
        let (r, sd) = (self.regs, self.sd);
        alloc::format!(
            "stream ctl {:#x} sts {:#x} lpib {} cbl {}, gctl {:#x} intsts {:#x}",
            rd32(r, sd + SD_CTL),
            rd8(r, sd + SD_STS),
            rd32(r, sd + SD_LPIB),
            rd32(r, sd + SD_CBL),
            rd32(r, GCTL),
            rd32(r, 0x24)
        )
    }
}

/// Bring up an HDA controller: the output stream if a codec has an
/// output path.
pub fn probe(dev: &pci::Device) -> Option<Output> {
    dev.enable();
    dev.set_intx_disabled(true);
    let bar = dev.bar(0);
    if bar == 0 {
        return None;
    }
    // Intel: traffic class 0 for the controller's DMA (TCSEL), as the
    // ICH data sheets ask, against clicks.
    if dev.vendor == 0x8086 {
        let v = dev.read(0x44);
        dev.write(0x44, v & !7);
    }
    let regs = mmio::map(bar, 0x4000);
    let gcap = rd16(regs, GCAP);
    let (iss, oss) = ((gcap >> 8 & 0xf) as usize, (gcap >> 12 & 0xf) as usize);
    let ok64 = gcap & 1 != 0;
    let name = alloc::format!("hda {}: {:04x}:{:04x} HDA {}.{}", dev.bdf(), dev.vendor, dev.device, rd8(regs, VMAJ), rd8(regs, VMIN));
    if oss == 0 {
        crate::println!("{name}: no output streams");
        return None;
    }
    // Controller reset: CRST low, then high; the codecs then announce
    // themselves in STATESTS.
    wr32(regs, INTCTL, 0);
    wr8(regs, CORBCTL, 0);
    wr8(regs, RIRBCTL, 0);
    wr32(regs, GCTL, rd32(regs, GCTL) & !1);
    if !wait(regs, 100, || rd32(regs, GCTL) & 1 == 0) {
        crate::println!("{name}: does not enter reset");
        return None;
    }
    msi::poll(1, || {}, || false);
    wr32(regs, GCTL, rd32(regs, GCTL) | 1);
    if !wait(regs, 100, || rd32(regs, GCTL) & 1 != 0) {
        crate::println!("{name}: does not leave reset");
        return None;
    }
    wait(regs, 100, || rd16(regs, STATESTS) & 0x7fff != 0);
    // Give late codecs the rest of their 521 us.
    msi::poll(2, || {}, || false);
    let codecs = rd16(regs, STATESTS) & 0x7fff;
    wr16(regs, STATESTS, codecs);
    if codecs == 0 {
        crate::println!("{name}: no codecs");
        return None;
    }
    // DMA memory below 4 GB unless the controller does 64-bit
    // addresses: CORB (1 KB) and RIRB (2 KB) on one page, the BDL on
    // another, then the ring.
    let limit = if ok64 { u64::MAX } else { 0xffff_ffff };
    let pages = 2 + RING_BYTES / PAGE;
    let mem = phys::alloc_contig(pages as u64, limit)?;
    let (corb, rirb, bdl, ring) = (mem, mem + 1024, mem + PAGE as u64, mem + 2 * PAGE as u64);
    let mut link = Link { regs, rings: setup_rings(regs, corb, rirb), rirb_rp: 0 };
    let mut out = None;
    for cad in 0..15u32 {
        if codecs & 1 << cad == 0 {
            continue;
        }
        let Some(id) = link.cmd(cad, 0, GET_PARAM << 8 | P_VENDOR) else {
            crate::println!("{name}: codec {cad} does not answer");
            continue;
        };
        let Some(g) = read_group(&mut link, cad) else {
            crate::println!("{name}: codec {cad} {:04x}:{:04x}: no audio function group", id >> 16, id & 0xffff);
            continue;
        };
        let paths = route(&mut link, &g);
        crate::println!(
            "{name}: codec {cad} {:04x}:{:04x}, function group {:#x}, {} widgets: {}",
            id >> 16,
            id & 0xffff,
            g.afg,
            g.widgets.len(),
            if paths.is_empty() { String::from("no output path") } else { paths.join(", ") }
        );
        if !paths.is_empty() && out.is_none() {
            out = Some(());
        }
    }
    out?;
    // The buffer descriptor list: the ring in equal parts.
    let part = RING_BYTES / BDL_ENTRIES;
    let b = phys::phys_to_virt(bdl) as *mut u32;
    for i in 0..BDL_ENTRIES {
        let a = ring + (i * part) as u64;
        unsafe {
            write_volatile(b.add(i * 4), a as u32);
            write_volatile(b.add(i * 4 + 1), (a >> 32) as u32);
            write_volatile(b.add(i * 4 + 2), part as u32);
            write_volatile(b.add(i * 4 + 3), 0);
        }
    }
    let sd = SD_BASE + iss * 0x20;
    let o = Output { regs, sd, bdl, ring, running: false };
    o.reset_stream();
    Some(o)
}

/// Program and start the CORB and RIRB; `None` if they cannot be set up
/// (the immediate command registers are used then).
fn setup_rings(regs: *mut u8, corb: u64, rirb: u64) -> Option<(*mut u32, *mut u32, usize, usize)> {
    fn size(cap: u8) -> Option<(u8, usize)> {
        [(2u8, 256usize), (1, 16), (0, 2)].into_iter().find(|&(code, _)| cap >> 4 & 1 << code != 0)
    }
    let (cs, nc) = size(rd8(regs, CORBSIZE))?;
    let (rs, nr) = size(rd8(regs, RIRBSIZE))?;
    wr8(regs, CORBSIZE, (rd8(regs, CORBSIZE) & !3) | cs);
    wr8(regs, RIRBSIZE, (rd8(regs, RIRBSIZE) & !3) | rs);
    wr32(regs, CORBLBASE, corb as u32);
    wr32(regs, CORBUBASE, (corb >> 32) as u32);
    wr32(regs, RIRBLBASE, rirb as u32);
    wr32(regs, RIRBUBASE, (rirb >> 32) as u32);
    // Read pointer reset: some controllers show the bit until it is
    // cleared, others clear it at once.
    wr16(regs, CORBRP, 0x8000);
    wait(regs, 1, || rd16(regs, CORBRP) & 0x8000 != 0);
    wr16(regs, CORBRP, 0);
    wait(regs, 1, || rd16(regs, CORBRP) & 0x8000 == 0);
    wr16(regs, CORBWP, 0);
    wr16(regs, RIRBWP, 0x8000);
    // A response interrupt after every response (the controller's
    // interrupt itself stays off, INTCTL.GIE clear): the status flag it
    // sets is what lets the next command through.
    wr16(regs, RINTCNT, 1);
    wr8(regs, CORBCTL, 2);
    wr8(regs, RIRBCTL, 3);
    let v = |p: u64| phys::phys_to_virt(p) as *mut u32;
    Some((v(corb), v(rirb), nc, nr))
}
