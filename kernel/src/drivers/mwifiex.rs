//! Marvell AVASTAR 88W8897 WiFi on PCIe (11AB:2B38), the Surface Pro 4's
//! WiFi (docs/SURFACE-PRO-4.md, "Phase 4: WiFi"). The reference is Linux
//! `drivers/net/wireless/marvell/mwifiex` (`pcie.c`, `pcie.h`); the
//! command format is in `crates/wlan` (`wlan::mwifiex`).
//!
//! The chip is full-MAC: its firmware (`pcie8897_uapsta.bin` from
//! linux-firmware, `C:\RS2\FIRMWARE\PCIE8897.BIN` on rs2) scans,
//! associates and encrypts; the host sends commands and 802.3 frames.
//!
//! * Registers are in BAR 2. The firmware asks for each block of the
//!   image by writing its length to a scratch register; the host puts the
//!   block in a DMA buffer, writes its address and size, and rings the
//!   doorbell ([`Card::download`]).
//! * After that the host tells the firmware where its descriptor rings
//!   are (PCIE_DESC_DETAILS): 32 transmit and 32 receive descriptors (the
//!   8897's "PFU" layout) and 8 event descriptors, each pointing at a
//!   buffer. A command goes through the same scratch registers as the
//!   firmware blocks, its response into a buffer whose address the host
//!   gave; events and received frames arrive in the rings.
//! * Interrupts are not used yet: the driver polls the host interrupt
//!   status register while it waits for a command (everything so far is
//!   command-driven: start-up and scans).
//!
//! Power saving stays off (the firmware is known to crash with it on, and
//! the Surface then needs a D3cold cycle of the slot to recover).

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};

use wlan::ie::Bss;
use wlan::mwifiex as mw;

use super::pci;
use super::virtio::queue::Dma;
use crate::mm::mmio;
use crate::net::wlan::{ScanRequest, WlanCaps, WlanDriver, WlanError, WlanEvent};
use crate::sync::{Mutex, SpinLock};

pub const FIRMWARE: &str = "C:\\RS2\\FIRMWARE\\PCIE8897.BIN";

const VENDOR_MARVELL: u16 = 0x11ab;
const DEVICE_88W8897: u16 = 0x2b38;

pub fn is_mwifiex(d: &pci::Device) -> bool {
    d.vendor == VENDOR_MARVELL && d.device == DEVICE_88W8897
}

// Registers (BAR 2) of the 8897 (`mwifiex_reg_8897`).
const CMD_ADDR_LO: usize = 0xc10;
const CMD_ADDR_HI: usize = 0xc14;
const CPU_INT_EVENT: usize = 0xc18;
const CPU_INT_STATUS: usize = 0xc1c;
const HOST_INT_STATUS: usize = 0xc30;
const HOST_INT_MASK: usize = 0xc34;
const HOST_INT_STATUS_MASK: usize = 0xc3c;
const CMD_SIZE: usize = 0xc40;
const FW_STATUS: usize = 0xc44;
const CMDRSP_ADDR_LO: usize = 0xcd0;
const CMDRSP_ADDR_HI: usize = 0xcd4;
const EVT_RDPTR: usize = 0xce8;
const EVT_WRPTR: usize = 0xcec;
const DRV_RDY: usize = 0xcf0;
const SCRATCH_13: usize = 0xcf4;
/// The host's receive read pointer (and transmit write pointer).
const WR_DATA_PTR: usize = 0xc05c;
/// The firmware's receive write pointer (and transmit read pointer).
const RD_DATA_PTR: usize = 0xc08c;
/// Chip revision: write 0x80c00000, read bits 8-15.
const REVISION: usize = 0xc58;
const BAR2_SIZE: u64 = 0x10000;

const CPU_INTR_DNLD_RDY: u32 = 1 << 0;
const CPU_INTR_DOOR_BELL: u32 = 1 << 1;
const CPU_INTR_EVENT_DONE: u32 = 1 << 5;
const HOST_INTR_DNLD_DONE: u32 = 1 << 0;
const HOST_INTR_UPLD_RDY: u32 = 1 << 1;
const HOST_INTR_CMD_DONE: u32 = 1 << 2;
const HOST_INTR_EVENT_RDY: u32 = 1 << 3;
const HOST_INTR_MASK: u32 = HOST_INTR_DNLD_DONE | HOST_INTR_UPLD_RDY | HOST_INTR_CMD_DONE | HOST_INTR_EVENT_RDY;

const FIRMWARE_READY: u32 = 0xfedc_ba00;
const FLR_HAPPENS: u32 = 0xfedc_baba;

const TXRX_BD: usize = 32;
const EVT_BD: usize = 8;
/// PFU descriptor: flags, offset, frag_len, len (u16), address (u64), reserved.
const PFU_DESC: usize = 20;
/// Event descriptor: address (u64), len, flags (u16).
const EVT_DESC: usize = 12;
const TX_MASK: u32 = 0x03ff_0000;
const TX_WRAP_MASK: u32 = 0x07ff_0000;
const TX_ROLLOVER: u32 = 1 << 26;
const RX_MASK: u32 = 0x3ff;
const RX_ROLLOVER: u32 = 1 << 10;
const EVT_MASK: u32 = 0xf;
const EVT_ROLLOVER: u32 = 1 << 7;
const FLAG_SOP: u16 = 1 << 0;
const FLAG_EOP: u16 = 1 << 1;
const FLAG_FIRST_DESC: u16 = 1 << 0;
const FLAG_LAST_DESC: u16 = 1 << 1;

const RX_BUF: usize = 4096;
const EVT_BUF: usize = 2048;
/// The largest command, response or firmware block (`MWIFIEX_UPLD_SIZE`).
const UPLD_SIZE: usize = 2312;
/// Firmware blocks go in multiples of this.
const FW_BLOCK: usize = 256;
const CMD_BUF: usize = 4096;
const TX_BUF_SIZE: u16 = 4096;

struct Regs(*mut u8);

unsafe impl Send for Regs {}
unsafe impl Sync for Regs {}

impl Regs {
    fn r(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.0.add(off) as *const u32) }
    }

    fn w(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.0.add(off) as *mut u32, v) }
    }
}

fn put16(d: &Dma, off: usize, v: u16) {
    d.write(off, &v.to_le_bytes());
}

fn put64(d: &Dma, off: usize, v: u64) {
    d.write(off, &v.to_le_bytes());
}

fn get16(d: &Dma, off: usize) -> u16 {
    let b = d.bytes(off, 2);
    u16::from_le_bytes([b[0], b[1]])
}

/// The command and event side (used under [`Card::inner`]).
struct Rings {
    evtbd: Dma,
    evtbuf: Dma,
    cmd: Dma,
    rsp: Dma,
}

impl Rings {
    fn new() -> Option<Rings> {
        let r = Rings { evtbd: Dma::new(EVT_BD * EVT_DESC)?, evtbuf: Dma::new(EVT_BD * EVT_BUF)?, cmd: Dma::new(CMD_BUF)?, rsp: Dma::new(CMD_BUF)? };
        for i in 0..EVT_BD {
            r.arm_evt(i);
        }
        fence(Ordering::SeqCst);
        Some(r)
    }

    fn arm_evt(&self, i: usize) {
        let d = i * EVT_DESC;
        put64(&self.evtbd, d, self.evtbuf.addr(i * EVT_BUF));
        put16(&self.evtbd, d + 8, EVT_BUF as u16);
        put16(&self.evtbd, d + 10, 0);
    }
}

/// The data side: the transmit and receive rings, their buffers and the
/// ring pointers (both live in one register pair, so they share a lock).
/// Under a spin lock of its own: the network thread transmits while a
/// command (a scan) may hold [`Card::inner`] for seconds.
struct Data {
    txbd: Dma,
    txbuf: Dma,
    rxbd: Dma,
    rxbuf: Dma,
    tx_wr: u32,
    tx_rd: u32,
    /// Frames handed to the firmware and not yet taken back.
    tx_pending: usize,
    rx_rd: u32,
}

/// One transmit buffer: interface header, TxPD, frame.
const TX_BUF: usize = 2048;

impl Data {
    fn new() -> Option<Data> {
        let d = Data {
            txbd: Dma::new(TXRX_BD * PFU_DESC)?,
            txbuf: Dma::new(TXRX_BD * TX_BUF)?,
            rxbd: Dma::new(TXRX_BD * PFU_DESC)?,
            rxbuf: Dma::new(TXRX_BD * RX_BUF)?,
            tx_wr: 0,
            // As Linux: the firmware's read pointer starts at 0 with the
            // rollover bit set (PFU chips).
            tx_rd: TX_ROLLOVER,
            tx_pending: 0,
            rx_rd: RX_ROLLOVER,
        };
        d.txbd.write(0, &[0; TXRX_BD * PFU_DESC]);
        for i in 0..TXRX_BD {
            d.arm_rx(i);
        }
        fence(Ordering::SeqCst);
        Some(d)
    }

    /// The host's half of the shared pointer register.
    fn wr_ptrs(&self) -> u32 {
        self.rx_rd | (self.tx_wr & TX_WRAP_MASK)
    }

    /// At most TXRX_BD - 1 frames in flight, so that the firmware's read
    /// index alone says how many it has taken (whatever its rollover bit
    /// means: on the 88W8897 it starts set, and comparing it the other
    /// way made the ring look full forever, so nothing was ever sent).
    fn tx_full(&self) -> bool {
        self.tx_pending >= TXRX_BD - 1
    }

    /// Take back the buffers the firmware has sent (its read pointer is in
    /// the high half of RD_DATA_PTR).
    fn reclaim(&mut self, r: &Regs) {
        let rd = r.r(RD_DATA_PTR);
        if rd == u32::MAX {
            return;
        }
        while self.tx_pending > 0 && (self.tx_rd & TX_MASK) != (rd & TX_MASK) {
            let i = ((self.tx_rd & TX_MASK) >> 16) as usize;
            if i < TXRX_BD {
                self.txbd.write(i * PFU_DESC, &[0; PFU_DESC]);
            }
            self.tx_rd += 1 << 16;
            if (self.tx_rd & TX_MASK) >> 16 == TXRX_BD as u32 {
                self.tx_rd = (self.tx_rd & TX_ROLLOVER) ^ TX_ROLLOVER;
            }
            self.tx_pending -= 1;
            TX_DONE.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn tx_empty(&self) -> bool {
        self.tx_pending == 0
    }

    fn arm_rx(&self, i: usize) {
        let d = i * PFU_DESC;
        put16(&self.rxbd, d, FLAG_SOP | FLAG_EOP);
        put16(&self.rxbd, d + 2, 0);
        put16(&self.rxbd, d + 4, RX_BUF as u16);
        put16(&self.rxbd, d + 6, RX_BUF as u16);
        put64(&self.rxbd, d + 8, self.rxbuf.addr(i * RX_BUF));
        self.rxbd.write(d + 16, &[0; 4]);
    }
}

struct Inner {
    rings: Option<Rings>,
    up: bool,
    seq: u8,
    evt_rd: u32,
    /// Events received and not yet handled: (cause, body).
    events: Vec<(u32, Vec<u8>)>,
    /// The firmware refused the legacy scan command: use SCAN_EXT.
    ext_scan: bool,
}

pub struct Card {
    name: String,
    dev: pci::Device,
    regs: Regs,
    inner: Mutex<Inner>,
    mac: SpinLock<[u8; 6]>,
    fw_version: SpinLock<String>,
    revision: AtomicU32,
    rx_frames: AtomicU64,
    events: AtomicU64,
    /// A scan has run (the first one logs each step).
    scanned: core::sync::atomic::AtomicBool,
    data: SpinLock<Option<Data>>,
    /// Received frames (Ethernet II) not yet taken by [`WlanDriver::poll`].
    rxq: SpinLock<VecDeque<Vec<u8>>>,
    /// KEY_MATERIAL layout (the firmware's key API major version).
    key_api: AtomicU32,
    /// The network joined (for DEAUTHENTICATE).
    bssid: SpinLock<Option<[u8; 6]>>,
    tx_frames: AtomicU64,
    tx_dropped: AtomicU64,
}

/// MSI interrupts taken (the handler only wakes the wlan thread).
static IRQS: AtomicU64 = AtomicU64::new(0);
/// Transmitted frames the firmware has handed back.
static TX_DONE: AtomicU64 = AtomicU64::new(0);

fn interrupt(_ctx: usize) {
    IRQS.fetch_add(1, Ordering::Relaxed);
    crate::net::wlan::kick();
}

fn delay_ms(ms: u64) {
    super::virtio::wait(ms, || {}, || false);
}

fn now_us() -> u64 {
    crate::rtc::mono_scaled(1_000_000)
}

/// Poll `cond` for up to `us` microseconds, without sleeping.
fn spin_until(us: u64, mut cond: impl FnMut() -> bool) -> bool {
    let t0 = now_us();
    loop {
        if cond() {
            return true;
        }
        if now_us() - t0 > us {
            return cond();
        }
        core::hint::spin_loop();
    }
}

/// Put the function in D0 if the firmware left it in D3hot. Leaving D3hot
/// may reset its configuration (no No_Soft_Reset): the BARs are written
/// back.
fn power_up(d: &pci::Device) {
    let Some(pm) = d.find_cap(1) else { return };
    let pmcsr = d.read(pm + 4);
    if pmcsr & 3 == 0 {
        return;
    }
    let bars: Vec<u32> = (0..6).map(|i| d.read(0x10 + i * 4)).collect();
    let cmd = d.read(0x04) & 0xffff;
    d.write(pm + 4, pmcsr & !0x8003);
    delay_ms(10);
    for (i, b) in bars.iter().enumerate() {
        d.write(0x10 + i as u8 * 4, *b);
    }
    d.write(0x04, cmd);
    crate::wlog!("mwifiex {}: was in D{}, now D0", d.bdf(), pmcsr & 3);
}

/// Found while the PCI bus is probed: map the registers and register with
/// the WiFi layer; [`Card::start`] runs later on the wlan thread.
pub fn probe(d: &pci::Device) {
    if !is_mwifiex(d) {
        return;
    }
    let name = alloc::format!("88W8897 {}", d.bdf());
    power_up(d);
    d.enable();
    // Polled for now: keep the INTx pin quiet.
    d.set_intx_disabled(true);
    let base = d.bar(2);
    if base == 0 || d.read(0x18) & 1 != 0 {
        crate::wlog!("{name}: BAR 2 is not memory ({:#x})", d.read(0x18));
        return;
    }
    let regs = Regs(mmio::map(base, BAR2_SIZE));
    let card = Card {
        name,
        dev: *d,
        regs,
        inner: Mutex::new(Inner { rings: None, up: false, seq: 0, evt_rd: EVT_ROLLOVER, events: Vec::new(), ext_scan: false }),
        mac: SpinLock::new([0; 6]),
        fw_version: SpinLock::new(String::new()),
        revision: AtomicU32::new(0),
        rx_frames: AtomicU64::new(0),
        events: AtomicU64::new(0),
        scanned: core::sync::atomic::AtomicBool::new(false),
        data: SpinLock::new(None),
        rxq: SpinLock::new(VecDeque::new()),
        key_api: AtomicU32::new(0),
        bssid: SpinLock::new(None),
        tx_frames: AtomicU64::new(0),
        tx_dropped: AtomicU64::new(0),
    };
    // The interrupt only wakes the wlan thread, which reads the status;
    // without MSI that thread polls.
    match crate::msi::setup(d, false, interrupt, 0) {
        Some((v, kind)) => crate::wlog!("{}: {kind} vector {v:#x}", card.name),
        None => crate::wlog!("{}: no MSI; polled", card.name),
    }
    crate::net::wlan::register(Arc::new(card));
}

impl Card {
    /// The firmware runs (it answers the driver-ready handshake). Polls
    /// up to `tries` times, 100 ms apart.
    fn fw_ready(&self, tries: u32) -> bool {
        let r = &self.regs;
        r.w(HOST_INT_STATUS_MASK, HOST_INTR_MASK);
        r.w(DRV_RDY, FIRMWARE_READY);
        for t in 0..tries {
            if r.r(FW_STATUS) == FIRMWARE_READY {
                return true;
            }
            if t + 1 < tries {
                crate::task::sched::sleep_ms(100);
            }
        }
        false
    }

    /// Send the firmware image block by block as the card asks for it.
    fn download(&self, rings: &Rings, fw: &[u8]) -> Result<(), WlanError> {
        let r = &self.regs;
        r.w(HOST_INT_MASK, 0);
        if r.r(SCRATCH_13) == FLR_HAPPENS {
            // After a function reset the card wants only the WiFi part of
            // a combo image; PCIE8897.BIN is WiFi only (starts with a
            // download command 1), so it goes as it is.
            crate::wlog!("{}: card was function-reset", self.name);
        }
        let t0 = crate::irq::uptime_ms();
        let (mut offset, mut blocks, mut retries, mut sent) = (0usize, 0usize, 0, 0usize);
        while offset < fw.len() {
            // The card writes the size of the block it wants.
            let mut len = 0;
            spin_until(200_000, || {
                len = r.r(CMD_SIZE) as usize;
                len != 0
            });
            if len == 0 {
                crate::wlog!("{}: card stopped asking for firmware at {offset} of {}", self.name, fw.len());
                break;
            }
            if len > UPLD_SIZE {
                return Err(WlanError::Failed(alloc::format!("card asked for a {len}-byte firmware block at {offset}")));
            }
            let txlen = if len & 1 != 0 {
                // CRC error: the same block again.
                retries += 1;
                if retries > 2 {
                    return Err(WlanError::Failed(alloc::format!("firmware block at {offset} failed its CRC three times")));
                }
                0
            } else {
                retries = 0;
                let n = len.min(fw.len() - offset);
                blocks = n.div_ceil(FW_BLOCK);
                rings.cmd.write(0, &fw[offset..offset + n]);
                rings.cmd.write(n, &alloc::vec![0u8; blocks * FW_BLOCK - n]);
                n
            };
            fence(Ordering::SeqCst);
            let pa = rings.cmd.addr(0);
            r.w(CMD_ADDR_LO, pa as u32);
            r.w(CMD_ADDR_HI, (pa >> 32) as u32);
            r.w(CMD_SIZE, (blocks * FW_BLOCK) as u32);
            r.w(CPU_INT_EVENT, CPU_INTR_DOOR_BELL);
            if !spin_until(500_000, || r.r(CPU_INT_STATUS) & CPU_INTR_DOOR_BELL == 0) {
                return Err(WlanError::Timeout("firmware block"));
            }
            offset += txlen;
            sent += 1;
        }
        crate::wlog!("{}: firmware sent ({} bytes, {sent} blocks, {} ms)", self.name, offset, crate::irq::uptime_ms() - t0);
        Ok(())
    }

    /// Look at the host interrupt status: take received frames and events,
    /// and return the causes seen.
    fn service(&self, g: &mut Inner) -> u32 {
        let r = &self.regs;
        let ireg = r.r(HOST_INT_STATUS);
        if ireg == 0 || ireg == u32::MAX {
            return 0;
        }
        r.w(HOST_INT_STATUS, !ireg);
        if ireg & HOST_INTR_UPLD_RDY != 0 {
            self.drain_rx();
        }
        if ireg & HOST_INTR_EVENT_RDY != 0 {
            self.drain_events(g);
        }
        ireg
    }

    /// Take the received packets into [`Card::rxq`] as Ethernet frames.
    fn drain_rx(&self) {
        let r = &self.regs;
        let mut got = Vec::new();
        {
            let mut dg = self.data.lock();
            let Some(d) = dg.as_mut() else { return };
            for _ in 0..TXRX_BD * 2 {
                let wr = r.r(RD_DATA_PTR);
                let rd = d.rx_rd;
                if wr == u32::MAX || ((wr & RX_MASK) == (rd & RX_MASK) && (wr & RX_ROLLOVER) != (rd & RX_ROLLOVER)) {
                    break;
                }
                let i = (rd & RX_MASK) as usize;
                if i >= TXRX_BD {
                    break;
                }
                fence(Ordering::SeqCst);
                let len = (get16(&d.rxbuf, i * RX_BUF) as usize).clamp(mw::INTF_HDR, RX_BUF);
                let typ = get16(&d.rxbuf, i * RX_BUF + 2);
                if typ == mw::TYPE_DATA {
                    if let Some(f) = mw::rx_frame(d.rxbuf.bytes(i * RX_BUF + mw::INTF_HDR, len - mw::INTF_HDR)) {
                        got.push(f);
                    }
                }
                self.rx_frames.fetch_add(1, Ordering::Relaxed);
                d.arm_rx(i);
                fence(Ordering::SeqCst);
                let mut rd = rd + 1;
                if (rd & RX_MASK) as usize == TXRX_BD {
                    rd = (rd & RX_ROLLOVER) ^ RX_ROLLOVER;
                }
                d.rx_rd = rd;
                r.w(WR_DATA_PTR, d.wr_ptrs());
            }
        }
        if !got.is_empty() {
            let mut q = self.rxq.lock();
            for f in got {
                if q.len() < 256 {
                    q.push_back(f);
                }
            }
        }
    }

    fn drain_events(&self, g: &mut Inner) {
        let r = &self.regs;
        for _ in 0..EVT_BD * 2 {
            let Some(rings) = &g.rings else { return };
            let wr = r.r(EVT_WRPTR);
            let rd = g.evt_rd;
            if (wr & EVT_MASK) == (rd & EVT_MASK) && (wr & EVT_ROLLOVER) != (rd & EVT_ROLLOVER) {
                break;
            }
            let i = (rd & EVT_MASK) as usize;
            if i >= EVT_BD {
                break;
            }
            fence(Ordering::SeqCst);
            let len = (get16(&rings.evtbuf, i * EVT_BUF) as usize).clamp(mw::INTF_HDR, EVT_BUF);
            let b = rings.evtbuf.bytes(i * EVT_BUF + mw::INTF_HDR, len - mw::INTF_HDR);
            if let Some((cause, body)) = mw::event(b) {
                self.events.fetch_add(1, Ordering::Relaxed);
                if g.events.len() < 64 {
                    g.events.push((cause, body.to_vec()));
                }
            }
            rings.arm_evt(i);
            fence(Ordering::SeqCst);
            let mut rd = rd + 1;
            if (rd & EVT_MASK) as usize == EVT_BD {
                rd = (rd & EVT_ROLLOVER) ^ EVT_ROLLOVER;
            }
            g.evt_rd = rd;
            r.w(EVT_RDPTR, rd);
        }
        r.w(CPU_INT_EVENT, CPU_INTR_EVENT_DONE);
    }

    /// Send a command and wait for its response body.
    fn command(&self, g: &mut Inner, cmd: u16, body: &[u8], timeout_ms: u64) -> Result<Vec<u8>, WlanError> {
        let name = mw::command_name(cmd);
        g.seq = g.seq.wrapping_add(1);
        let c = mw::command(cmd, g.seq, body);
        let total = mw::INTF_HDR + c.len();
        let r = &self.regs;
        {
            let Some(rings) = &g.rings else { return Err(WlanError::NotReady) };
            put16(&rings.cmd, 0, total as u16);
            put16(&rings.cmd, 2, mw::TYPE_CMD);
            rings.cmd.write(mw::INTF_HDR, &c);
            rings.rsp.write(0, &[0; 8]);
            fence(Ordering::SeqCst);
            let (pc, pr) = (rings.cmd.addr(0), rings.rsp.addr(0));
            r.w(CMDRSP_ADDR_LO, pr as u32);
            r.w(CMDRSP_ADDR_HI, (pr >> 32) as u32);
            r.w(CMD_ADDR_LO, pc as u32);
            r.w(CMD_ADDR_HI, (pc >> 32) as u32);
            r.w(CMD_SIZE, total as u32);
            r.w(CPU_INT_EVENT, CPU_INTR_DOOR_BELL);
        }
        let t0 = crate::irq::uptime_ms();
        loop {
            if self.service(g) & HOST_INTR_CMD_DONE != 0 {
                break;
            }
            if crate::irq::uptime_ms() - t0 > timeout_ms {
                crate::wlog!("{}: {name} timed out (host int status {:#x}, fw status {:#x})", self.name, r.r(HOST_INT_STATUS), r.r(FW_STATUS));
                return Err(WlanError::Timeout(name));
            }
            crate::task::sched::sleep_ms(1);
        }
        r.w(CMDRSP_ADDR_LO, 0);
        r.w(CMDRSP_ADDR_HI, 0);
        fence(Ordering::SeqCst);
        let rings = g.rings.as_ref().ok_or(WlanError::NotReady)?;
        let len = (get16(&rings.rsp, 0) as usize).clamp(mw::INTF_HDR, CMD_BUF);
        let b = rings.rsp.bytes(mw::INTF_HDR, len - mw::INTF_HDR);
        let resp = mw::response(b).ok_or_else(|| WlanError::Failed(alloc::format!("{name}: no response ({len} bytes)")))?;
        if resp.command != cmd {
            return Err(WlanError::Failed(alloc::format!("{name}: response to {:#x}", resp.command)));
        }
        if resp.result != 0 {
            return Err(WlanError::Refused(name, resp.result));
        }
        if !g.up {
            // Start-up: every step in the log until the chip has worked once.
            crate::wlog!("{}: {name} done ({} bytes, {} ms)", self.name, resp.body.len(), crate::irq::uptime_ms() - t0);
        }
        Ok(resp.body.to_vec())
    }

    fn init(&self, g: &mut Inner) -> Result<String, WlanError> {
        let r = &self.regs;
        r.w(REVISION, 0x80c0_0000);
        let rev = r.r(REVISION) & 0xff00;
        self.revision.store(rev, Ordering::Relaxed);
        let rev_name = match rev {
            0x1100 => "A0",
            0x1200 => "B0",
            _ => "?",
        };
        crate::wlog!(
            "{}: BAR 2 at {:#x}, revision {rev:#x} ({rev_name}), firmware status {:#x}, scratch 13 {:#x}, host interrupt status {:#x}",
            self.name,
            self.dev.bar(2),
            r.r(FW_STATUS),
            r.r(SCRATCH_13),
            r.r(HOST_INT_STATUS)
        );
        if g.rings.is_none() {
            g.rings = Some(Rings::new().ok_or(WlanError::Failed("out of memory".into()))?);
        }
        if self.data.lock().is_none() {
            let d = Data::new().ok_or(WlanError::Failed("out of memory".into()))?;
            *self.data.lock() = Some(d);
        }
        if self.fw_ready(1) {
            crate::wlog!("{}: firmware already running", self.name);
        } else {
            // Zero: this function downloads (the 8897's Bluetooth half on
            // USB could be doing it with a combo image).
            let winner = r.r(FW_STATUS) == 0;
            if winner {
                if rev == 0x1100 {
                    crate::wlog!("{}: revision A0 needs pcie8897_uapsta_a0.bin; trying the B0 image", self.name);
                }
                let fw = crate::fs::read_file(FIRMWARE).ok_or_else(|| WlanError::NoFirmware(FIRMWARE.into()))?;
                self.download(g.rings.as_ref().unwrap(), &fw)?;
            } else {
                crate::wlog!("{}: firmware download by the other function (status {:#x}); waiting", self.name, r.r(FW_STATUS));
            }
            if !self.fw_ready(150) {
                return Err(WlanError::Timeout("firmware start"));
            }
        }
        r.w(HOST_INT_MASK, HOST_INTR_MASK);
        let details = {
            let rings = g.rings.as_ref().unwrap();
            let dg = self.data.lock();
            let d = dg.as_ref().unwrap();
            mw::pcie_desc_details((d.txbd.base, TXRX_BD as u32), (d.rxbd.base, TXRX_BD as u32), (rings.evtbd.base, EVT_BD as u32), 0)
        };
        self.command(g, mw::CMD_PCIE_DESC_DETAILS, &details, 2000)?;
        self.command(g, mw::CMD_FUNC_INIT, &[], 5000)?;
        let spec_body = self.command(g, mw::CMD_GET_HW_SPEC, &[0; mw::HW_SPEC_LEN], 2000)?;
        let spec = mw::HwSpec::parse(&spec_body).ok_or(WlanError::Failed("short GET_HW_SPEC response".into()))?;
        let (kmaj, kmin) = mw::key_api_version(&spec_body);
        self.key_api.store(kmaj as u32, Ordering::Relaxed);
        crate::wlog!("{}: key API {kmaj}.{kmin}", self.name);
        *self.mac.lock() = spec.mac;
        *self.fw_version.lock() = spec.fw_version();
        let mut txb = Vec::new();
        for v in [1u16, TX_BUF_SIZE, 0, 0] {
            txb.extend(v.to_le_bytes());
        }
        if let Err(e) = self.command(g, mw::CMD_RECONFIGURE_TX_BUFF, &txb, 2000) {
            crate::wlog!("{}: {e}", self.name);
        }
        let mac_ctl = mw::MAC_RX_ON | mw::MAC_TX_ON | mw::MAC_ETHERNETII;
        self.command(g, mw::CMD_MAC_CONTROL, &mac_ctl.to_le_bytes(), 2000)?;
        // Let the firmware deliver received frames.
        if let Some(d) = self.data.lock().as_ref() {
            r.w(WR_DATA_PTR, d.wr_ptrs());
        }
        g.up = true;
        Ok(alloc::format!(
            "revision {rev_name}, firmware {}, MAC {}, region {:#x}, {} antenna(s), capabilities {:#x}",
            spec.fw_version(),
            wlan::MacFmt(spec.mac),
            spec.region,
            spec.antennas,
            spec.fw_cap
        ))
    }

    /// One scan command; the networks found.
    fn scan_group(&self, g: &mut Inner, chans: &[mw::ScanChannel]) -> Result<Vec<Bss>, WlanError> {
        if !g.ext_scan {
            match self.command(g, mw::CMD_802_11_SCAN, &mw::scan_body(chans, 1, false), 5000) {
                Ok(body) => {
                    return mw::parse_scan_response(&body).ok_or_else(|| WlanError::Failed(alloc::format!("unparsable scan response ({} bytes)", body.len())));
                }
                Err(WlanError::Refused(_, code)) => {
                    crate::wlog!("{}: legacy scan refused ({code:#x}); using the extended scan", self.name);
                    g.ext_scan = true;
                }
                Err(e) => return Err(e),
            }
        }
        g.events.retain(|e| e.0 != mw::EVENT_EXT_SCAN_REPORT);
        self.command(g, mw::CMD_802_11_SCAN_EXT, &mw::scan_body(chans, 1, true), 5000)?;
        let mut found = Vec::new();
        let t0 = crate::irq::uptime_ms();
        loop {
            let mut done = false;
            for (cause, body) in core::mem::take(&mut g.events) {
                if cause == mw::EVENT_EXT_SCAN_REPORT {
                    if let Some((v, more)) = mw::parse_ext_scan_report(&body) {
                        found.extend(v);
                        done |= !more;
                    }
                } else {
                    g.events.push((cause, body));
                }
            }
            if done {
                return Ok(found);
            }
            let dwell: u64 = chans.iter().map(|c| c.time_ms as u64).sum();
            if crate::irq::uptime_ms() - t0 > dwell + 3000 {
                return Err(WlanError::Timeout("extended scan report"));
            }
            crate::task::sched::sleep_ms(5);
            self.service(g);
        }
    }

}

impl WlanDriver for Card {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn caps(&self) -> WlanCaps {
        WlanCaps { band_2ghz: true, band_5ghz: true, full_mac: true }
    }

    fn mac_address(&self) -> [u8; 6] {
        *self.mac.lock()
    }

    fn start(&self) -> Result<String, WlanError> {
        let mut g = self.inner.lock();
        let r = self.init(&mut g);
        if r.is_err() {
            crate::wlog!(
                "{}: start failed: fw status {:#x}, host int status {:#x}, PCI command/status {:#x}",
                self.name,
                self.regs.r(FW_STATUS),
                self.regs.r(HOST_INT_STATUS),
                self.dev.read(0x04)
            );
        }
        r
    }

    fn scan(&self, _req: &ScanRequest) -> Result<Vec<Bss>, WlanError> {
        let mut g = self.inner.lock();
        if !g.up {
            return Err(WlanError::NotReady);
        }
        let found = self.scan_all(&mut g)?;
        if found.is_empty() && !g.ext_scan {
            // Linux always uses the extended scan on this chip: if the
            // legacy one finds nothing, try that before believing it.
            crate::wlog!("{}: the legacy scan found nothing; trying the extended scan", self.name);
            g.ext_scan = true;
            return self.scan_all(&mut g);
        }
        Ok(found)
    }

    fn diagnostics(&self) -> Vec<(String, String)> {
        alloc::vec![
            ("firmware".into(), self.fw_version.lock().clone()),
            ("revision".into(), alloc::format!("{:#x}", self.revision.load(Ordering::Relaxed))),
            ("events".into(), alloc::format!("{}", self.events.load(Ordering::Relaxed))),
            ("rx_frames".into(), alloc::format!("{}", self.rx_frames.load(Ordering::Relaxed))),
            ("tx_frames".into(), alloc::format!("{}", self.tx_frames.load(Ordering::Relaxed))),
            ("tx_done".into(), alloc::format!("{}", TX_DONE.load(Ordering::Relaxed))),
            ("tx_dropped".into(), alloc::format!("{}", self.tx_dropped.load(Ordering::Relaxed))),
            ("interrupts".into(), alloc::format!("{}", IRQS.load(Ordering::Relaxed))),
            ("key_api".into(), alloc::format!("{}", self.key_api.load(Ordering::Relaxed))),
        ]
    }

    fn connect(&self, bss: &Bss, rsn_ie: Option<&[u8]>) -> Result<u16, WlanError> {
        let mut g = self.inner.lock();
        if !g.up {
            return Err(WlanError::NotReady);
        }
        let body = self.command(&mut g, mw::CMD_802_11_ASSOCIATE, &mw::associate_body(bss, rsn_ie), 15000)?;
        let (cap, status, aid) = mw::associate_result(&body).ok_or(WlanError::Failed("short association response".into()))?;
        if status != 0 {
            let why = match cap {
                1 => "authentication failed",
                2 => "no answer to authentication",
                3 => "association timed out",
                4 => "association refused",
                5 => "station failure",
                _ => "refused by the access point",
            };
            return Err(WlanError::Failed(alloc::format!("{why} (status {status}, {cap:#x})")));
        }
        *self.bssid.lock() = Some(bss.bssid);
        Ok(aid)
    }

    fn disconnect(&self) {
        let Some(b) = self.bssid.lock().take() else { return };
        let mut g = self.inner.lock();
        if let Err(e) = self.command(&mut g, mw::CMD_802_11_DEAUTHENTICATE, &mw::deauth_body(b, 3), 3000) {
            crate::wlog!("{}: deauthenticate: {e}", self.name);
        }
    }

    fn set_key(&self, key: &mw::Key) -> Result<(), WlanError> {
        let mut g = self.inner.lock();
        let body = mw::key_material_body(key, self.key_api.load(Ordering::Relaxed) as u8);
        self.command(&mut g, mw::CMD_802_11_KEY_MATERIAL, &body, 3000).map(|_| ())
    }

    fn send(&self, frame: &[u8]) -> bool {
        let total = mw::INTF_HDR + mw::TXPD_LEN + frame.len();
        if frame.len() < 14 || total > TX_BUF {
            return false;
        }
        let r = &self.regs;
        let mut dg = self.data.lock();
        let Some(d) = dg.as_mut() else { return false };
        if d.tx_full() {
            d.reclaim(r);
        }
        if d.tx_full() {
            self.tx_dropped.fetch_add(1, Ordering::Relaxed);
            // Tell the firmware again that there is something to send.
            r.w(CPU_INT_EVENT, CPU_INTR_DNLD_RDY);
            return false;
        }
        let i = ((d.tx_wr & TX_MASK) >> 16) as usize;
        let at = i * TX_BUF;
        put16(&d.txbuf, at, total as u16);
        put16(&d.txbuf, at + 2, mw::TYPE_DATA);
        d.txbuf.write(at + mw::INTF_HDR, &mw::txpd(frame.len()));
        d.txbuf.write(at + mw::INTF_HDR + mw::TXPD_LEN, frame);
        let desc = i * PFU_DESC;
        put16(&d.txbd, desc, FLAG_FIRST_DESC | FLAG_LAST_DESC);
        put16(&d.txbd, desc + 2, 0);
        put16(&d.txbd, desc + 4, total as u16);
        put16(&d.txbd, desc + 6, total as u16);
        put64(&d.txbd, desc + 8, d.txbuf.addr(at));
        d.txbd.write(desc + 16, &[0; 4]);
        fence(Ordering::SeqCst);
        d.tx_pending += 1;
        d.tx_wr += 1 << 16;
        if (d.tx_wr & TX_MASK) >> 16 == TXRX_BD as u32 {
            d.tx_wr = (d.tx_wr & TX_ROLLOVER) ^ TX_ROLLOVER;
        }
        r.w(WR_DATA_PTR, d.tx_wr | (d.rx_rd & 0x7ff));
        // Linux: the 88W8897 firmware may crash after a write pointer
        // update with ASPM on unless the bus is kept busy with a read.
        let _ = r.r(0);
        r.w(CPU_INT_EVENT, CPU_INTR_DNLD_RDY);
        self.tx_frames.fetch_add(1, Ordering::Relaxed);
        true
    }

    fn flush_tx(&self, ms: u64) {
        let t0 = crate::irq::uptime_ms();
        loop {
            {
                let mut dg = self.data.lock();
                let Some(d) = dg.as_mut() else { return };
                d.reclaim(&self.regs);
                if d.tx_empty() {
                    return;
                }
            }
            if crate::irq::uptime_ms() - t0 > ms {
                return;
            }
            crate::task::sched::sleep_ms(1);
        }
    }

    fn poll(&self, f: &mut dyn FnMut(WlanEvent)) {
        let events = {
            let mut g = self.inner.lock();
            if !g.up {
                return;
            }
            self.service(&mut g);
            core::mem::take(&mut g.events)
        };
        if let Some(d) = self.data.lock().as_mut() {
            d.reclaim(&self.regs);
        }
        for (cause, body) in events {
            let reason = if body.len() >= 2 { u16::from_le_bytes([body[0], body[1]]) } else { 0 };
            let ev = match cause {
                mw::EVENT_LINK_LOST => WlanEvent::Disconnected { why: "link lost", reason },
                mw::EVENT_DEAUTHENTICATED => WlanEvent::Disconnected { why: "deauthenticated", reason },
                mw::EVENT_DISASSOCIATED => WlanEvent::Disconnected { why: "disassociated", reason },
                c => WlanEvent::Other(c),
            };
            if matches!(ev, WlanEvent::Disconnected { .. }) {
                *self.bssid.lock() = None;
            }
            f(ev);
        }
        loop {
            let Some(fr) = self.rxq.lock().pop_front() else { break };
            f(WlanEvent::Frame(fr));
        }
    }
}

impl Card {
    fn scan_all(&self, g: &mut Inner) -> Result<Vec<Bss>, WlanError> {
        // Up to four channels per command, one band and kind at a time
        // (the response buffer holds about 2 KB of beacons).
        let first = !self.scanned.swap(true, Ordering::Relaxed);
        let chans = mw::default_channels();
        let mut found = Vec::new();
        let mut i = 0;
        while i < chans.len() {
            let mut j = i + 1;
            while j < chans.len() && j - i < 4 && chans[j].band == chans[i].band && chans[j].passive == chans[i].passive {
                j += 1;
            }
            match self.scan_group(g, &chans[i..j]) {
                Ok(v) => {
                    if first {
                        crate::wlog!("{}: channels {}-{}{}: {} network(s)", self.name, chans[i].channel, chans[j - 1].channel, if chans[i].passive { " (passive)" } else { "" }, v.len());
                    }
                    found.extend(v)
                }
                Err(e @ WlanError::Timeout(_)) => return Err(e),
                Err(e) => crate::wlog!("{}: scan of channels {}-{}: {e}", self.name, chans[i].channel, chans[j - 1].channel),
            }
            i = j;
        }
        Ok(found)
    }
}
