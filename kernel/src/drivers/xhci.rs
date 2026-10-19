//! xHCI (USB 3) host controllers: root ports, hubs, device enumeration,
//! the interrupt-IN transfers of HID boot keyboards and mice and of hubs,
//! and the bulk transfers of mass storage devices.
//!
//! Bring-up follows the xHCI specification: BIOS hand-off through the USB
//! Legacy Support capability, halt and reset, scratchpad buffers, the
//! device context base address array, a command ring and one event ring
//! (interrupter 0, one segment). At boot the event ring and the port
//! status registers are polled in a loop until the devices present are
//! configured. Afterwards interrupter 0 interrupts through MSI-X (or
//! MSI): the handler does what the tick would (events, port scan,
//! timers, hubs) if it gets the controller lock, and wakes threads
//! waiting for transfers. The timer tick then only polls while an
//! enumeration is in progress and every SCAN_MS (hot-plug safety net,
//! besides port status change events); without MSI it polls every
//! POLL_TICKS as before.
//!
//! Enumeration is asynchronous and event driven so that the same code runs
//! at boot and in the tick: every device (on a root port or behind a hub)
//! has an enumeration step that advances when its command or control
//! transfer completes. Hubs run the port state machine of `hub`; devices
//! behind them get their route string, root port and, for low and full
//! speed devices behind a high-speed hub, its transaction translator in
//! the slot context.
//!
//! Mass storage transfers are synchronous instead and run in the thread
//! doing the I/O (`storage`): [`bulk`] and [`control`] queue the TRBs under
//! the controller lock, then poll the event ring themselves, dropping the
//! lock and sleeping until the next interrupt between polls (yielding
//! without one). The tick and the interrupt handler only `try_lock`, so
//! they skip a round while a thread polls; whoever sees the completion
//! event stores it in the device, where the waiting thread picks it up.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::hub::{self, Hub};
use super::pci;
use super::storage;
use super::usb::{self, speed, Setup};
use super::hid;
use crate::fs::block::BlockDev;
use crate::mm::{mmio, phys};
use crate::msi::{self, Event as IrqEvent};
use crate::sync::SpinLock;

const PAGE: usize = 4096;
/// TRBs per ring segment (one page); the last one is the link TRB.
const TRBS: usize = 256;
/// Pages per device: input context, output context, data buffer, EP0
/// ring, then the rings of the other endpoints (interrupt or bulk).
const DEV_PAGES: usize = 4 + MAX_HID;
/// HID interfaces used per device.
const MAX_HID: usize = 4;
/// Offset of the interrupt transfer buffers in the data page (HID
/// reports, then the hub status-change bitmap).
const REPORT_BUF: usize = 2048;
const REPORT_MAX: usize = 64;
const HUB_BUF: usize = REPORT_BUF + MAX_HID * REPORT_MAX;
/// Time allowed for one command or control transfer.
const STEP_TIMEOUT_MS: u64 = 1000;
/// Boot-time wait for devices to appear and be configured (hubs take a
/// while: power-on, debounce and reset of every port, one device at a
/// time).
const BOOT_MIN_MS: u64 = 250;
const BOOT_MAX_MS: u64 = 8000;
/// Hub tiers below the root ports that the route string can describe.
const MAX_DEPTH: u8 = 5;
/// Poll interval (ticks) and port scan interval (ms) after boot.
const POLL_TICKS: u64 = 4;
const SCAN_MS: u64 = 100;
/// Interrupt endpoint errors tolerated before a device is given up.
const MAX_EP_ERRORS: u32 = 16;
/// Reports traced to the serial port per device (evidence that input
/// arrives over USB).
const TRACE_REPORTS: u32 = 12;

// Capability registers.
const CAPLENGTH: usize = 0x00;
const HCSPARAMS1: usize = 0x04;
const HCSPARAMS2: usize = 0x08;
const HCCPARAMS1: usize = 0x10;
const DBOFF: usize = 0x14;
const RTSOFF: usize = 0x18;

// Operational registers.
const USBCMD: usize = 0x00;
const USBSTS: usize = 0x04;
const PAGESIZE: usize = 0x08;
const CRCR: usize = 0x18;
const DCBAAP: usize = 0x30;
const CONFIG: usize = 0x38;
const PORTSC: usize = 0x400;

const CMD_RUN: u32 = 1 << 0;
const CMD_HCRST: u32 = 1 << 1;
const STS_HCH: u32 = 1 << 0;
const STS_HSE: u32 = 1 << 2;
const STS_EINT: u32 = 1 << 3;
const CMD_INTE: u32 = 1 << 2;
const IMAN_IP: u32 = 1 << 0;
const IMAN_IE: u32 = 1 << 1;
const STS_CNR: u32 = 1 << 11;
const STS_HCE: u32 = 1 << 12;

// Interrupter 0 registers (from the runtime base).
const IMAN: usize = 0x20;
const IMOD: usize = 0x24;
const ERSTSZ: usize = 0x28;
const ERSTBA: usize = 0x30;
const ERDP: usize = 0x38;

// PORTSC bits.
const PORT_CCS: u32 = 1 << 0;
const PORT_PED: u32 = 1 << 1;
const PORT_PR: u32 = 1 << 4;
const PORT_PP: u32 = 1 << 9;
const PORT_CSC: u32 = 1 << 17;
const PORT_PRC: u32 = 1 << 21;
/// All RW1C change bits (CSC PEC WRC OCC PRC PLC CEC).
const PORT_CHANGES: u32 = 0x7f << 17;
/// Bits written back unchanged: PP, port indicator, wake enables.
const PORT_PRESERVE: u32 = PORT_PP | 3 << 14 | 7 << 25;

// TRB types.
const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_DISABLE_SLOT: u32 = 10;
const TRB_ADDRESS: u32 = 11;
const TRB_CONFIGURE: u32 = 12;
const TRB_EVALUATE: u32 = 13;
const TRB_RESET_EP: u32 = 14;
const TRB_STOP_EP: u32 = 15;
const TRB_SET_DEQ: u32 = 16;
const EV_TRANSFER: u32 = 32;
const EV_COMMAND: u32 = 33;
const EV_PORT: u32 = 34;
const EV_HOST: u32 = 37;

// TRB control bits.
const TRB_CYCLE: u32 = 1 << 0;
const TRB_TC: u32 = 1 << 1;
const TRB_ISP: u32 = 1 << 2;
const TRB_CH: u32 = 1 << 4;
const TRB_IOC: u32 = 1 << 5;
const TRB_IDT: u32 = 1 << 6;
const TRB_DIR_IN: u32 = 1 << 16;

// Completion codes.
const CC_SUCCESS: u8 = 1;
const CC_STALL: u8 = 6;
const CC_SHORT: u8 = 13;
const CC_STOPPED: u8 = 26;
const CC_STOPPED_LENGTH: u8 = 27;

// Endpoint types (endpoint context).
const EP_BULK_OUT: u32 = 2;
const EP_CONTROL: u32 = 4;
const EP_BULK_IN: u32 = 6;
const EP_INTERRUPT_IN: u32 = 7;

fn rd32(base: *mut u8, off: usize) -> u32 {
    unsafe { read_volatile(base.add(off) as *const u32) }
}

fn wr32(base: *mut u8, off: usize, v: u32) {
    unsafe { write_volatile(base.add(off) as *mut u32, v) }
}

fn wr64(base: *mut u8, off: usize, v: u64) {
    wr32(base, off, v as u32);
    wr32(base, off + 4, (v >> 32) as u32);
}

/// Milliseconds that also advance with interrupts disabled (during boot):
/// while the tick count stands still the PIT channel 0 counter, which runs
/// as the tick source, is sampled instead. Each sample counts at least a
/// microsecond, in case the PIT is not running.
struct Clock {
    ticks: u64,
    pit: u16,
    us: u64,
}

const PIT_HZ: u64 = 1_193_182;

fn pit_counter() -> u16 {
    unsafe {
        crate::arch::outb(0x43, 0x00); // latch channel 0
        let lo = crate::arch::inb(0x40) as u16;
        let hi = crate::arch::inb(0x40) as u16;
        hi << 8 | lo
    }
}

impl Clock {
    fn new() -> Clock {
        Clock { ticks: crate::irq::ticks(), pit: pit_counter(), us: 0 }
    }

    fn now_ms(&mut self) -> u64 {
        let t = crate::irq::ticks();
        if t != self.ticks {
            self.us += (t - self.ticks) * 1_000_000 / crate::irq::HZ;
            self.ticks = t;
        } else {
            let div = (PIT_HZ / crate::irq::HZ) as u32;
            let now = pit_counter();
            let d = (self.pit as u32 + div - now as u32) % div;
            self.pit = now;
            self.us += (d as u64 * 1_000_000 / PIT_HZ).max(1);
        }
        self.us / 1000
    }

    /// Busy-wait up to `ms` for `cond`.
    fn wait(&mut self, ms: u64, mut cond: impl FnMut() -> bool) -> bool {
        let end = self.now_ms() + ms;
        while self.now_ms() < end {
            if cond() {
                return true;
            }
            core::hint::spin_loop();
        }
        cond()
    }
}

/// A producer ring (command or transfer) of one segment closed by a link
/// TRB that toggles the cycle state.
struct Ring {
    base: u64,
    idx: usize,
    cycle: bool,
}

impl Ring {
    /// Ring in the zeroed page at `base`.
    fn new(base: u64) -> Ring {
        let link = unsafe { phys::phys_to_virt(base).add((TRBS - 1) * 16) as *mut u32 };
        unsafe {
            write_volatile(link, base as u32);
            write_volatile(link.add(1), (base >> 32) as u32);
            write_volatile(link.add(3), TRB_LINK << 10 | TRB_TC);
        }
        Ring { base, idx: 0, cycle: true }
    }

    /// Enqueue one TRB (the cycle bit is added); returns its address.
    fn push(&mut self, param: u64, status: u32, control: u32) -> u64 {
        let addr = self.base + (self.idx * 16) as u64;
        let p = phys::phys_to_virt(addr) as *mut u32;
        let cycle = if self.cycle { TRB_CYCLE } else { 0 };
        unsafe {
            write_volatile(p, param as u32);
            write_volatile(p.add(1), (param >> 32) as u32);
            write_volatile(p.add(2), status);
            fence(Ordering::SeqCst);
            write_volatile(p.add(3), control | cycle);
        }
        self.idx += 1;
        if self.idx == TRBS - 1 {
            // Hand the link TRB over and wrap; it is part of a TD that
            // continues past it.
            let link = unsafe { phys::phys_to_virt(self.base).add((TRBS - 1) * 16) as *mut u32 };
            let chain = control & TRB_CH;
            unsafe { write_volatile(link.add(3), TRB_LINK << 10 | TRB_TC | chain | cycle) };
            self.idx = 0;
            self.cycle = !self.cycle;
        }
        fence(Ordering::SeqCst);
        addr
    }

    /// Dequeue pointer for Set TR Dequeue Pointer: the next free TRB.
    fn enqueue_ptr(&self) -> u64 {
        self.base + (self.idx * 16) as u64 | self.cycle as u64
    }
}

/// An event TRB.
#[derive(Clone, Copy)]
struct Event {
    param: u64,
    status: u32,
    control: u32,
}

impl Event {
    fn kind(&self) -> u32 {
        (self.control >> 10) & 0x3f
    }

    fn code(&self) -> u8 {
        (self.status >> 24) as u8
    }

    fn slot(&self) -> u8 {
        (self.control >> 24) as u8
    }

    fn endpoint(&self) -> u8 {
        ((self.control >> 16) & 0x1f) as u8
    }
}

/// Enumeration and run state of a device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// Connect debounce before the port reset (root ports).
    Debounce(u64),
    Reset,
    /// Reset recovery.
    Settle(u64),
    EnableSlot,
    Address,
    DevDesc8,
    Evaluate,
    DevDesc,
    ConfigHead,
    Config,
    SetConfig,
    SetProtocol(u8),
    SetIdle(u8),
    HubDesc,
    HubDepth,
    Configure,
    /// Recovering EP0 after a stall, then continuing the saved step.
    Ep0Reset,
    Ep0Dequeue,
    Running,
    /// Enumerated, no driver.
    Unused,
    Failed,
}

/// A HID boot interface in use.
struct Hid {
    iface: usb::HidIface,
    kind: hid::Kind,
    dci: u8,
    ring: Ring,
    len: u16,
    handle: u32,
    errors: u32,
}

/// A hub: its port state machine and status-change endpoint.
struct HubDev {
    hub: Hub,
    ep: usb::Endpoint,
    ring: Ring,
    /// Device index behind each downstream port.
    children: [Option<usize>; hub::MAX_PORTS as usize + 1],
    errors: u32,
}

/// A mass storage (Bulk-Only) interface and its bulk endpoints.
struct Msc {
    iface: u8,
    ep_in: usb::Endpoint,
    ep_out: usb::Endpoint,
    ring_in: Ring,
    ring_out: Ring,
}

/// A transfer started from thread context, and its outcome.
struct SyncXfer {
    dci: u8,
    /// TRB addresses and data lengths of the TD (control transfers:
    /// setup, data, status).
    trbs: Vec<(u64, u32)>,
    /// Bytes moved when a short packet ended the data early.
    short: Option<usize>,
    /// Completion code and bytes transferred.
    done: Option<(u8, usize)>,
}

struct Device {
    /// Distinguishes this device from a later one in the same place.
    epoch: u32,
    speed: u8,
    slot: u8,
    step: Step,
    /// Step to continue after EP0 stall recovery.
    resume: Step,
    deadline: u64,
    /// DEV_PAGES contiguous pages, 0 until a slot is enabled.
    mem: u64,
    ep0: Ring,
    mps0: u16,
    /// Status stage TRB of the control transfer in flight.
    status_trb: u64,
    desc: usb::DeviceDesc,
    config: u8,
    /// Root hub port (1-based) the device is reached through.
    root_port: u8,
    /// Route string: the hub port of each tier, one nibble per tier.
    route: u32,
    /// Hubs between the root port and the device.
    depth: u8,
    /// Parent hub (device index) and its port.
    parent: Option<(usize, u8)>,
    /// Transaction translator (high-speed hub slot and port) of a low or
    /// full speed device behind a high-speed hub; 0 if none.
    tt: (u8, u8),
    hids: Vec<Hid>,
    hub: Option<HubDev>,
    msc: Option<Msc>,
    /// LED state last sent; a SET_REPORT (or hub request) is in flight.
    leds: u8,
    ctl_busy: bool,
    reports: u32,
    sync: Option<SyncXfer>,
}

impl Device {
    fn new(epoch: u32, root_port: u8, step: Step) -> Device {
        Device {
            epoch,
            speed: 0,
            slot: 0,
            step,
            resume: Step::Failed,
            deadline: 0,
            mem: 0,
            ep0: Ring { base: 0, idx: 0, cycle: true },
            mps0: 8,
            status_trb: 0,
            desc: usb::DeviceDesc::default(),
            config: 0,
            root_port,
            route: 0,
            depth: 0,
            parent: None,
            tt: (0, 0),
            hids: Vec::new(),
            hub: None,
            msc: None,
            leds: 0,
            ctl_busy: false,
            reports: 0,
            sync: None,
        }
    }

    fn page(&self, n: usize) -> u64 {
        self.mem + (n * PAGE) as u64
    }

    fn in_ctx(&self) -> u64 {
        self.page(0)
    }

    fn out_ctx(&self) -> u64 {
        self.page(1)
    }

    fn data(&self) -> u64 {
        self.page(2)
    }

    fn data_slice(&self, len: usize) -> &[u8] {
        unsafe { core::slice::from_raw_parts(phys::phys_to_virt(self.data()), len.min(REPORT_BUF)) }
    }

    /// The transfer ring of endpoint `dci`.
    fn ring(&mut self, dci: u8) -> Option<&mut Ring> {
        if dci == 1 {
            return Some(&mut self.ep0);
        }
        if let Some(n) = self.hids.iter().position(|h| h.dci == dci) {
            return Some(&mut self.hids[n].ring);
        }
        if self.hub.as_ref().is_some_and(|h| h.ep.dci() == dci) {
            return self.hub.as_mut().map(|h| &mut h.ring);
        }
        let m = self.msc.as_mut()?;
        if m.ep_in.dci() == dci {
            Some(&mut m.ring_in)
        } else if m.ep_out.dci() == dci {
            Some(&mut m.ring_out)
        } else {
            None
        }
    }

    /// Whether `dci` is a bulk endpoint (used only from thread context).
    fn is_bulk(&self, dci: u8) -> bool {
        self.msc.as_ref().is_some_and(|m| m.ep_in.dci() == dci || m.ep_out.dci() == dci)
    }

    /// "1" for root port 1, "1.3.2" behind hubs.
    fn path(&self) -> String {
        let mut s = String::new();
        let _ = write!(s, "{}", self.root_port);
        for t in 0..self.depth {
            let _ = write!(s, ".{}", (self.route >> (4 * t)) & 0xf);
        }
        s
    }
}

/// Commands in flight, with what they were for.
#[derive(Clone, Copy)]
enum Cmd {
    /// Part of the device's enumeration (its current step).
    Step,
    /// Interrupt endpoint `dci` recovery after an error.
    EpReset(u8),
    EpDequeue(u8),
    /// Slot of a removed device; its memory is freed on completion.
    DisableSlot(u64),
    /// Issued from thread context; the completion code is kept in
    /// `sync_cmds`.
    Sync,
}

struct Ctrl {
    name: String,
    /// Position in CTRLS.
    index: usize,
    op: *mut u8,
    rt: *mut u8,
    db: *mut u8,
    /// Context size: 32 or 64 bytes.
    csz: usize,
    ports: usize,
    /// Physical address limit for DMA memory (no 64-bit addressing).
    limit: u64,
    dcbaa: u64,
    cmd: Ring,
    ev_base: u64,
    ev_idx: usize,
    ev_cycle: bool,
    cmds: Vec<(u64, usize, u32, Cmd)>,
    /// All devices; `root` and the hubs' children index into this.
    devs: Vec<Option<Device>>,
    /// Device on each root port.
    root: Vec<Option<usize>>,
    next_epoch: u32,
    clock: Clock,
    last_scan: u64,
    dead: bool,
    booting: bool,
    /// Mass storage devices configured during boot, set up once the
    /// controller is registered.
    new_storage: Vec<usize>,
    /// Completion codes of thread-context commands, by TRB address.
    sync_cmds: Vec<(u64, u8)>,
    /// Signalled by interrupter 0's interrupt, if it has one.
    irq: Option<&'static IrqEvent>,
    /// A port status change event arrived: scan the ports now.
    port_event: bool,
}

/// What the interrupt handler needs without the controller lock.
struct IrqCtx {
    index: usize,
    op: *mut u8,
    rt: *mut u8,
    event: IrqEvent,
}

unsafe impl Sync for IrqCtx {}

/// Interrupter 0: acknowledge, process the event ring like the tick does
/// (if no thread holds the controller), wake transfer waiters.
fn interrupt(ctx: usize) {
    let x = unsafe { &*(ctx as *const IrqCtx) };
    wr32(x.op, USBSTS, STS_EINT);
    wr32(x.rt, IMAN, IMAN_IE | IMAN_IP);
    if let Some(mut cs) = CTRLS.try_lock() {
        if let Some(c) = cs.get_mut(x.index) {
            c.poll();
        }
    }
    x.event.signal();
}

unsafe impl Send for Ctrl {}

static CTRLS: SpinLock<Vec<Ctrl>> = SpinLock::new(Vec::new());

/// Size of memory BAR 0 (sized with decoding off).
fn bar_size(dev: &pci::Device) -> u64 {
    let cmd = dev.read(0x04);
    dev.write(0x04, cmd & !0x2);
    let lo = dev.read(0x10);
    dev.write(0x10, 0xffff_ffff);
    let mask = dev.read(0x10) & !0xf;
    dev.write(0x10, lo);
    dev.write(0x04, cmd);
    if mask == 0 {
        0x10000
    } else {
        (!mask as u64 + 1) & 0xffff_ffff
    }
}

/// Take the controller from the BIOS (USB Legacy Support capability) and
/// switch off its SMIs.
fn bios_handoff(cap: *mut u8, clock: &mut Clock) {
    let mut off = ((rd32(cap, HCCPARAMS1) >> 16) as usize) * 4;
    while off != 0 {
        let v = rd32(cap, off);
        if v & 0xff == 1 {
            if v & (1 << 16) != 0 {
                unsafe { write_volatile(cap.add(off + 3), 1u8) };
                let ok = clock.wait(1000, || rd32(cap, off) & (1 << 16) == 0);
                if !ok {
                    crate::println!("xhci: BIOS did not release the controller, taking it");
                    unsafe { write_volatile(cap.add(off + 2), 0u8) };
                }
            } else {
                unsafe { write_volatile(cap.add(off + 3), 1u8) };
            }
            let c = rd32(cap, off + 4);
            wr32(cap, off + 4, (c & !(1 | 1 << 4 | 7 << 13)) | 7 << 29);
        }
        let next = ((v >> 8) & 0xff) as usize;
        if next == 0 {
            break;
        }
        off += next * 4;
    }
}

/// Intel 7/8/9-series chipsets power up with their ports routed to the
/// EHCI controllers; route every port that can switch to the xHCI
/// (XUSB2PR/USB3_PSSEN from their masks XUSB2PRM/USB3PRM).
fn intel_port_routing(dev: &pci::Device) {
    const SWITCHABLE: [u16; 5] = [0x1e31, 0x8c31, 0x9c31, 0x8cb1, 0x9cb1];
    if dev.vendor != 0x8086 || !SWITCHABLE.contains(&dev.device) {
        return;
    }
    let usb3 = dev.read(0xdc) & 0xf;
    dev.write(0xd8, (dev.read(0xd8) & !0xf) | usb3);
    let usb2 = dev.read(0xd4) & 0x7fff;
    dev.write(0xd0, (dev.read(0xd0) & !0x7fff) | usb2);
}

fn alloc_pages(n: usize, limit: u64) -> Option<u64> {
    phys::alloc_contig(n as u64, limit)
}


/// Initialise the controller, configure the devices present and set up
/// the mass storage devices among them. Returns their logical units as
/// block devices, with whether each is an optical drive.
pub fn probe(dev: &pci::Device) -> Vec<(Arc<dyn BlockDev>, bool)> {
    let Some(mut c) = start(dev) else { return Vec::new() };
    // Interrupter 0 through MSI-X entry 0 or MSI; the messages flow once
    // the kernel is up (msi::start), boot enumeration polls.
    let ctx: &'static IrqCtx = alloc::boxed::Box::leak(alloc::boxed::Box::new(IrqCtx {
        index: CTRLS.lock().len(),
        op: c.op,
        rt: c.rt,
        event: IrqEvent::new(),
    }));
    if let Some((v, kind)) = msi::setup(dev, true, interrupt, ctx as *const IrqCtx as usize) {
        ctx.event.arm();
        c.irq = Some(&ctx.event);
        wr32(c.rt, IMAN, IMAN_IE | IMAN_IP);
        wr32(c.op, USBCMD, rd32(c.op, USBCMD) | CMD_INTE);
        crate::println!("{}: {kind} vector {v:#x}", c.name);
    }
    c.booting = true;
    c.boot_enumerate();
    c.booting = false;
    let pending = core::mem::take(&mut c.new_storage);
    let idx = {
        let mut cs = CTRLS.lock();
        c.index = cs.len();
        cs.push(c);
        cs.len() - 1
    };
    let mut out = Vec::new();
    for i in pending {
        if let Some(info) = storage_info(idx, i) {
            out.extend(storage::attach(info));
        }
    }
    out
}

/// Bring the controller up: reset, memory structures, run, port power.
fn start(dev: &pci::Device) -> Option<Ctrl> {
    dev.enable();
    // Polled: mask legacy INTx.
    dev.write(0x04, (dev.read(0x04) & 0xffff) | 1 << 10);
    let bar = dev.bar(0);
    if bar == 0 {
        return None;
    }
    let size = bar_size(dev).max(0x1000);
    let cap = mmio::map(bar, size);
    let mut clock = Clock::new();
    let caplen = (rd32(cap, CAPLENGTH) & 0xff) as usize;
    let hcs1 = rd32(cap, HCSPARAMS1);
    let hcs2 = rd32(cap, HCSPARAMS2);
    let hcc1 = rd32(cap, HCCPARAMS1);
    let max_slots = (hcs1 & 0xff) as usize;
    let ports = (hcs1 >> 24) as usize;
    let op = unsafe { cap.add(caplen) };
    let rt = unsafe { cap.add((rd32(cap, RTSOFF) & !0x1f) as usize) };
    let db = unsafe { cap.add((rd32(cap, DBOFF) & !0x3) as usize) };
    let csz = if hcc1 & (1 << 2) != 0 { 64 } else { 32 };
    let limit = if hcc1 & 1 != 0 { u64::MAX } else { 1 << 32 };
    let name = alloc::format!("xhci {:02x}:{:02x}.{}", dev.bus, dev.dev, dev.func);

    bios_handoff(cap, &mut clock);
    intel_port_routing(dev);
    // Halt, then reset.
    if rd32(op, USBSTS) & STS_HCH == 0 {
        wr32(op, USBCMD, rd32(op, USBCMD) & !CMD_RUN);
        if !clock.wait(100, || rd32(op, USBSTS) & STS_HCH != 0) {
            crate::println!("{name}: does not halt");
            return None;
        }
    }
    wr32(op, USBCMD, CMD_HCRST);
    // Some controllers must not be touched right after the reset.
    clock.wait(2, || false);
    if !clock.wait(1000, || rd32(op, USBCMD) & CMD_HCRST == 0 && rd32(op, USBSTS) & STS_CNR == 0) {
        crate::println!("{name}: reset timed out");
        return None;
    }
    let page_bits = rd32(op, PAGESIZE) & 0xffff;
    let hc_page = PAGE << page_bits.trailing_zeros().min(15);
    let scratch = (((hcs2 >> 21) & 0x1f) << 5 | (hcs2 >> 27) & 0x1f) as usize;

    // DCBAA, command ring, event ring, ERST.
    let mem = alloc_pages(4, limit)?;
    let dcbaa = mem;
    let cmd_ring = mem + PAGE as u64;
    let ev_base = mem + 2 * PAGE as u64;
    let erst = mem + 3 * PAGE as u64;
    if scratch > 0 {
        let per = hc_page / PAGE;
        let arr_pages = (scratch * 8).div_ceil(PAGE);
        // Over-allocate to align the buffers to the controller page size.
        let arr = alloc_pages(arr_pages, limit)?;
        let bufs = alloc_pages(scratch * per + per - 1, limit)?;
        let first = (bufs + hc_page as u64 - 1) & !(hc_page as u64 - 1);
        let a = phys::phys_to_virt(arr) as *mut u64;
        for i in 0..scratch {
            unsafe { write_volatile(a.add(i), first + (i * hc_page) as u64) };
        }
        unsafe { write_volatile(phys::phys_to_virt(dcbaa) as *mut u64, arr) };
    }
    unsafe {
        let e = phys::phys_to_virt(erst) as *mut u32;
        write_volatile(e, ev_base as u32);
        write_volatile(e.add(1), (ev_base >> 32) as u32);
        write_volatile(e.add(2), TRBS as u32);
    }
    let slots = max_slots.min(255);
    wr32(op, CONFIG, (rd32(op, CONFIG) & !0xff) | slots as u32);
    wr64(op, DCBAAP, dcbaa);
    let cmd = Ring::new(cmd_ring);
    wr64(op, CRCR, cmd_ring | 1);
    wr32(rt, IMAN, 0);
    wr32(rt, IMOD, 0);
    wr32(rt, ERSTSZ, 1);
    wr64(rt, ERDP, ev_base);
    wr64(rt, ERSTBA, erst);
    fence(Ordering::SeqCst);
    wr32(op, USBCMD, CMD_RUN);
    if !clock.wait(100, || rd32(op, USBSTS) & STS_HCH == 0) {
        crate::println!("{name}: does not start");
        return None;
    }
    crate::println!("{name}: {ports} ports, {slots} slots, {csz}-byte contexts, {scratch} scratchpads");

    let c = Ctrl {
        name,
        op,
        rt,
        db,
        csz,
        ports,
        limit,
        dcbaa,
        cmd,
        ev_base,
        ev_idx: 0,
        ev_cycle: true,
        cmds: Vec::new(),
        devs: Vec::new(),
        root: (0..ports).map(|_| None).collect(),
        next_epoch: 1,
        clock,
        last_scan: 0,
        dead: false,
        booting: false,
        new_storage: Vec::new(),
        sync_cmds: Vec::new(),
        index: 0,
        irq: None,
        port_event: false,
    };
    // Power the ports (when the controller switches port power).
    if hcc1 & (1 << 3) != 0 {
        for p in 0..ports {
            let sc = c.portsc(p);
            if sc & PORT_PP == 0 {
                c.port_write(p, PORT_PP);
            }
        }
    }
    Some(c)
}

/// Poll the controllers that need it; called from the timer interrupt.
/// One with an interrupt only while something is being enumerated and
/// every SCAN_MS.
pub fn tick() {
    let now = crate::irq::ticks();
    if now % POLL_TICKS != 0 {
        return;
    }
    let Some(mut cs) = CTRLS.try_lock() else { return };
    if cs.is_empty() {
        return;
    }
    let scan = now % (SCAN_MS * crate::irq::HZ / 1000) == 0;
    for c in cs.iter_mut() {
        if c.irq.is_none() || scan || c.busy() {
            c.poll();
        }
    }
    drop(cs);
    hid::tick();
}

impl Ctrl {
    fn portsc(&self, p: usize) -> u32 {
        rd32(self.op, PORTSC + p * 0x10)
    }

    /// Write PORTSC: preserved bits plus `bits` (RW1C change bits, PR...).
    fn port_write(&self, p: usize, bits: u32) {
        let v = (self.portsc(p) & PORT_PRESERVE) | bits;
        wr32(self.op, PORTSC + p * 0x10, v);
    }

    fn ring_doorbell(&self, slot: u8, target: u8) {
        fence(Ordering::SeqCst);
        wr32(self.db, slot as usize * 4, target as u32);
    }

    fn now(&mut self) -> u64 {
        self.clock.now_ms()
    }

    fn dev(&mut self, i: usize) -> &mut Device {
        self.devs[i].as_mut().unwrap()
    }

    /// Poll at boot until the connected devices are configured.
    fn boot_enumerate(&mut self) {
        let start = self.now();
        loop {
            self.poll_once(true);
            let t = self.now() - start;
            let busy = self.devs.iter().flatten().any(|d| {
                let hub_busy = d.step == Step::Running && d.hub.as_ref().is_some_and(|h| !h.hub.settled());
                hub_busy || !matches!(d.step, Step::Running | Step::Unused | Step::Failed)
            });
            if (t >= BOOT_MIN_MS && !busy) || t >= BOOT_MAX_MS || self.dead {
                break;
            }
            core::hint::spin_loop();
        }
    }

    fn poll(&mut self) {
        self.poll_once(false);
    }

    /// Enumeration or a request in progress: time-driven steps need the
    /// tick.
    fn busy(&self) -> bool {
        !self.cmds.is_empty()
            || self.devs.iter().flatten().any(|d| {
                let hub_busy = d.step == Step::Running && d.hub.as_ref().is_some_and(|h| !h.hub.settled());
                hub_busy || d.ctl_busy || !matches!(d.step, Step::Running | Step::Unused | Step::Failed)
            })
    }

    fn poll_once(&mut self, booting: bool) {
        if self.dead {
            return;
        }
        let now = self.now();
        if booting || now >= self.last_scan + SCAN_MS {
            self.last_scan = now;
            let sts = rd32(self.op, USBSTS);
            if sts & (STS_HSE | STS_HCE) != 0 {
                crate::println!("{}: host controller error (USBSTS {sts:#x}), stopped", self.name);
                self.dead = true;
                return;
            }
            self.scan_ports(now);
        }
        self.events();
        if self.port_event {
            self.port_event = false;
            self.scan_ports(now);
        }
        self.timers(now);
        self.hubs(now);
        self.leds();
    }

    /// Notice connects and disconnects on the root ports.
    fn scan_ports(&mut self, now: u64) {
        for p in 0..self.ports {
            let sc = self.portsc(p);
            if sc & PORT_CSC != 0 {
                self.port_write(p, PORT_CSC);
                if let Some(i) = self.root[p] {
                    self.remove(i);
                }
            }
            let connected = sc & PORT_CCS != 0;
            if !connected {
                if let Some(i) = self.root[p] {
                    self.remove(i);
                }
            }
            if connected && self.root[p].is_none() {
                let i = self.add(Step::Debounce(now + 100), (p + 1) as u8);
                self.root[p] = Some(i);
            }
        }
    }

    /// A new device entry in `step`; returns its index.
    fn add(&mut self, step: Step, root_port: u8) -> usize {
        let epoch = self.next_epoch;
        self.next_epoch += 1;
        let d = Device::new(epoch, root_port, step);
        match self.devs.iter().position(|d| d.is_none()) {
            Some(i) => {
                self.devs[i] = Some(d);
                i
            }
            None => {
                self.devs.push(Some(d));
                self.devs.len() - 1
            }
        }
    }

    /// A hub reset its port `port` and the device there is ready to be
    /// addressed.
    fn attach_child(&mut self, h: usize, port: u8, speed: u8) {
        let hd = self.dev(h);
        let (root_port, depth, route, hub_speed, hub_slot, hub_tt) = (hd.root_port, hd.depth, hd.route, hd.speed, hd.slot, hd.tt);
        let tt = match speed {
            speed::LOW | speed::FULL if hub_speed == speed::HIGH => (hub_slot, port),
            speed::LOW | speed::FULL => hub_tt,
            _ => (0, 0),
        };
        let now = self.now();
        let i = self.add(Step::Settle(now), root_port);
        let d = self.dev(i);
        d.speed = speed;
        d.depth = depth + 1;
        d.route = route | (port as u32) << (4 * depth);
        d.parent = Some((h, port));
        d.tt = tt;
        if let Some(hub) = self.dev(h).hub.as_mut() {
            hub.children[port as usize] = Some(i);
        }
    }

    fn detach_child(&mut self, h: usize, port: u8) {
        let child = self.dev(h).hub.as_mut().and_then(|hub| hub.children[port as usize].take());
        if let Some(c) = child {
            self.remove(c);
        }
    }

    /// Forget device `i` (and everything behind it) and release its slot.
    fn remove(&mut self, i: usize) {
        let Some(d) = self.devs[i].take() else { return };
        match d.parent {
            None => {
                let p = d.root_port as usize - 1;
                if self.root[p] == Some(i) {
                    self.root[p] = None;
                }
            }
            Some((h, port)) => {
                if let Some(hub) = self.devs[h].as_mut().and_then(|hd| hd.hub.as_mut()) {
                    if hub.children[port as usize] == Some(i) {
                        hub.children[port as usize] = None;
                    }
                }
            }
        }
        if let Some(hub) = &d.hub {
            for c in hub.children.iter().flatten() {
                self.remove(*c);
            }
        }
        for h in &d.hids {
            hid::detach(h.handle);
        }
        if matches!(d.step, Step::Running) {
            crate::println!("{}: port {} device removed", self.name, d.path());
            if d.msc.is_some() {
                STORAGE_EVENTS.post(StorageEvent::Removed(Handle { ctrl: self.index, dev: i, epoch: d.epoch }));
            }
        }
        if d.slot != 0 {
            let trb = self.cmd.push(0, 0, TRB_DISABLE_SLOT << 10 | (d.slot as u32) << 24);
            self.cmds.push((trb, i, 0, Cmd::DisableSlot(d.mem)));
            self.ring_doorbell(0, 0);
        } else if d.mem != 0 {
            free_pages(d.mem);
        }
    }

    fn fail(&mut self, i: usize, why: &str) {
        let name = self.name.clone();
        if let Some(d) = self.devs[i].as_mut() {
            crate::println!("{name}: port {}: {why} (step {:?})", d.path(), d.step);
            d.step = Step::Failed;
        }
    }

    /// Deadlines: debounce, reset completion, recovery, timeouts.
    fn timers(&mut self, now: u64) {
        for i in 0..self.devs.len() {
            let Some(d) = self.devs[i].as_ref() else { continue };
            let (step, deadline, p) = (d.step, d.deadline, d.root_port as usize - 1);
            let hub_waiting = d.hub.is_some() && d.ctl_busy;
            match step {
                Step::Debounce(t) if now >= t => {
                    let sc = self.portsc(p);
                    let d = self.dev(i);
                    if sc & PORT_PED != 0 {
                        // USB 3 ports enable themselves.
                        d.step = Step::Settle(now + 10);
                    } else {
                        d.step = Step::Reset;
                        d.deadline = now + 500;
                        self.port_write(p, PORT_PR);
                    }
                }
                Step::Reset => {
                    let sc = self.portsc(p);
                    if sc & PORT_PRC != 0 {
                        self.port_write(p, sc & PORT_CHANGES & !PORT_CSC);
                        if sc & PORT_PED != 0 {
                            self.dev(i).step = Step::Settle(now + 10);
                        } else {
                            self.fail(i, "port not enabled after reset");
                        }
                    } else if now >= deadline {
                        self.fail(i, "port reset timed out");
                    }
                }
                Step::Settle(t) if now >= t => self.enable_slot(i),
                Step::Running if hub_waiting && now >= deadline => self.fail(i, "hub request timed out"),
                Step::Debounce(_) | Step::Settle(_) | Step::Running | Step::Unused | Step::Failed => {}
                _ => {
                    if now >= deadline {
                        self.fail(i, "timed out");
                    }
                }
            }
        }
    }

    /// No device is at the default address: nothing between a port reset
    /// and the completion of Address Device behind a hub.
    fn may_reset(&self) -> bool {
        !self.devs.iter().flatten().any(|d| {
            let early = matches!(d.step, Step::Settle(_) | Step::EnableSlot | Step::Address);
            (d.parent.is_some() && early) || d.hub.as_ref().is_some_and(|h| h.hub.resetting())
        })
    }

    /// Advance the port state machines of the running hubs.
    fn hubs(&mut self, now: u64) {
        for i in 0..self.devs.len() {
            loop {
                let may_reset = self.may_reset();
                let Some(d) = self.devs[i].as_mut() else { break };
                if d.step != Step::Running || d.ctl_busy {
                    break;
                }
                let Some(h) = d.hub.as_mut() else { break };
                match h.hub.poll(now, may_reset) {
                    None => break,
                    Some(hub::Action::Control(s)) => {
                        d.ctl_busy = true;
                        self.control(i, s);
                        break;
                    }
                    Some(hub::Action::Attach { port, speed }) => self.attach_child(i, port, speed),
                    Some(hub::Action::Detach(port)) => self.detach_child(i, port),
                }
            }
        }
    }

    fn command(&mut self, i: usize, kind: Cmd, param: u64, control: u32) -> u64 {
        let epoch = self.devs[i].as_ref().map_or(0, |d| d.epoch);
        let trb = self.cmd.push(param, 0, control);
        self.cmds.push((trb, i, epoch, kind));
        let now = self.now();
        if let Some(d) = self.devs[i].as_mut() {
            if matches!(kind, Cmd::Step) {
                d.deadline = now + STEP_TIMEOUT_MS;
            }
        }
        self.ring_doorbell(0, 0);
        trb
    }

    /// Start a control transfer on EP0 with the data page as buffer;
    /// completion advances the step.
    fn control(&mut self, i: usize, s: Setup) {
        let now = self.now();
        let Some(d) = self.devs[i].as_mut() else { return };
        let len = s.length as usize;
        let data = d.data();
        if s.is_in() {
            unsafe { core::ptr::write_bytes(phys::phys_to_virt(data), 0, len.min(REPORT_BUF)) };
        }
        let (_, status) = push_control(&mut d.ep0, s, data);
        d.status_trb = status;
        d.deadline = now + STEP_TIMEOUT_MS;
        let slot = d.slot;
        self.ring_doorbell(slot, 1);
    }

    /// Consume the event ring.
    fn events(&mut self) {
        let mut any = false;
        loop {
            let p = phys::phys_to_virt(self.ev_base + (self.ev_idx * 16) as u64) as *const u32;
            let control = unsafe { read_volatile(p.add(3)) };
            if (control & TRB_CYCLE != 0) != self.ev_cycle {
                break;
            }
            fence(Ordering::SeqCst);
            let ev = unsafe {
                Event {
                    param: read_volatile(p) as u64 | (read_volatile(p.add(1)) as u64) << 32,
                    status: read_volatile(p.add(2)),
                    control,
                }
            };
            self.ev_idx += 1;
            if self.ev_idx == TRBS {
                self.ev_idx = 0;
                self.ev_cycle = !self.ev_cycle;
            }
            any = true;
            self.event(ev);
        }
        if any || self.irq.is_some() {
            // Update the dequeue pointer, clearing Event Handler Busy
            // (which blocks further interrupts until cleared).
            wr64(self.rt, ERDP, self.ev_base + (self.ev_idx * 16) as u64 | 1 << 3);
        }
    }

    fn event(&mut self, ev: Event) {
        match ev.kind() {
            EV_COMMAND => self.command_done(ev),
            EV_TRANSFER => self.transfer_done(ev),
            // The port registers are scanned (now, with interrupts).
            EV_PORT => self.port_event = true,
            EV_HOST => crate::println!("{}: host controller event {}", self.name, ev.code()),
            _ => {}
        }
    }

    fn command_done(&mut self, ev: Event) {
        let Some(n) = self.cmds.iter().position(|c| c.0 == ev.param) else { return };
        let (trb, i, epoch, kind) = self.cmds.remove(n);
        match kind {
            Cmd::DisableSlot(mem) => {
                if ev.slot() != 0 {
                    unsafe { write_volatile((phys::phys_to_virt(self.dcbaa) as *mut u64).add(ev.slot() as usize), 0) };
                }
                if mem != 0 {
                    free_pages(mem);
                }
                return;
            }
            Cmd::Sync => {
                self.sync_cmds.push((trb, ev.code()));
                return;
            }
            _ => {}
        }
        if !self.devs[i].as_ref().is_some_and(|d| d.epoch == epoch) {
            return; // the device is gone
        }
        match kind {
            Cmd::Step => self.step_command_done(i, ev),
            Cmd::EpReset(dci) => {
                let d = self.dev(i);
                let slot = d.slot;
                let Some(deq) = d.ring(dci).map(|r| r.enqueue_ptr()) else { return };
                let ctl = TRB_SET_DEQ << 10 | (slot as u32) << 24 | (dci as u32) << 16;
                self.command(i, Cmd::EpDequeue(dci), deq, ctl);
            }
            Cmd::EpDequeue(dci) => self.queue_interrupt(i, dci),
            Cmd::DisableSlot(_) | Cmd::Sync => {}
        }
    }

    fn step_command_done(&mut self, i: usize, ev: Event) {
        let step = self.dev(i).step;
        if ev.code() != CC_SUCCESS {
            let why = alloc::format!("command failed, completion code {}", ev.code());
            self.fail(i, &why);
            return;
        }
        match step {
            Step::EnableSlot => self.address_device(i, ev.slot()),
            Step::Address => self.get_device_desc8(i),
            Step::Evaluate => self.get_device_desc(i),
            Step::Configure => self.start_running(i),
            Step::Ep0Reset => {
                let d = self.dev(i);
                let ctl = TRB_SET_DEQ << 10 | (d.slot as u32) << 24 | 1 << 16;
                let deq = d.ep0.enqueue_ptr();
                d.step = Step::Ep0Dequeue;
                self.command(i, Cmd::Step, deq, ctl);
            }
            Step::Ep0Dequeue => {
                let d = self.dev(i);
                d.step = d.resume;
                if d.step == Step::Running {
                    // A stalled LED report (the keyboard has no LEDs) or
                    // hub request.
                    d.ctl_busy = false;
                    if let Some(h) = d.hub.as_mut() {
                        h.hub.failed();
                    }
                } else {
                    // A stalled optional request: carry on.
                    self.control_done(i);
                }
            }
            _ => {}
        }
    }

    fn transfer_done(&mut self, ev: Event) {
        let slot = ev.slot();
        let Some(i) = self.devs.iter().position(|d| d.as_ref().is_some_and(|d| d.slot == slot && slot != 0)) else {
            return;
        };
        let dci = ev.endpoint();
        if self.sync_event(i, dci, &ev) {
            return;
        }
        let code = ev.code();
        if dci == 1 {
            let d = self.dev(i);
            if code == CC_SUCCESS && ev.param == d.status_trb {
                self.control_done(i);
            } else if !matches!(code, CC_SUCCESS | CC_SHORT | CC_STOPPED | CC_STOPPED_LENGTH) {
                self.control_error(i, code);
            }
            return;
        }
        let ok = code == CC_SUCCESS || code == CC_SHORT;
        let d = self.dev(i);
        if d.hub.as_ref().is_some_and(|h| h.ep.dci() == dci) {
            if ok {
                self.hub_changed(i, &ev);
            } else {
                let h = d.hub.as_mut().unwrap();
                h.errors += 1;
                let errors = h.errors;
                self.endpoint_error(i, dci, errors);
            }
            return;
        }
        let Some(n) = d.hids.iter().position(|h| h.dci == dci) else { return };
        if ok {
            let data = d.data();
            let h = &mut d.hids[n];
            h.errors = 0;
            let got = (h.len as usize).saturating_sub((ev.status & 0xff_ffff) as usize);
            let off = REPORT_BUF + n * REPORT_MAX;
            let mut report = [0u8; REPORT_MAX];
            let src = unsafe { core::slice::from_raw_parts(phys::phys_to_virt(data).add(off), got) };
            report[..got].copy_from_slice(src);
            let (handle, kind) = (h.handle, h.kind);
            if d.reports < TRACE_REPORTS {
                d.reports += 1;
                crate::trace!("usb: slot {slot} {kind:?} report {:02x?}", &report[..got]);
            }
            self.queue_interrupt(i, dci);
            hid::report(handle, &report[..got]);
        } else {
            let h = &mut d.hids[n];
            h.errors += 1;
            let errors = h.errors;
            self.endpoint_error(i, dci, errors);
        }
    }

    /// An interrupt endpoint failed: give up after too many errors, else
    /// reset it (it halted) and skip the failed TD.
    fn endpoint_error(&mut self, i: usize, dci: u8, errors: u32) {
        if errors > MAX_EP_ERRORS {
            self.fail(i, "too many interrupt endpoint errors");
            return;
        }
        let slot = self.dev(i).slot;
        let ctl = TRB_RESET_EP << 10 | (slot as u32) << 24 | (dci as u32) << 16;
        self.command(i, Cmd::EpReset(dci), 0, ctl);
    }

    /// The hub's status-change endpoint reported a bitmap.
    fn hub_changed(&mut self, i: usize, ev: &Event) {
        let d = self.dev(i);
        let data = d.data();
        let Some(h) = d.hub.as_mut() else { return };
        h.errors = 0;
        let len = h.ep.max_packet.clamp(1, 8) as usize;
        let got = len.saturating_sub((ev.status & 0xff_ffff) as usize);
        let bitmap = unsafe { core::slice::from_raw_parts(phys::phys_to_virt(data).add(HUB_BUF), got) };
        h.hub.changed(bitmap);
        let dci = h.ep.dci();
        self.queue_interrupt(i, dci);
    }

    /// EP0 failed. Optional requests may stall; anything else fails the
    /// device.
    fn control_error(&mut self, i: usize, code: u8) {
        let d = self.dev(i);
        let tolerable = matches!(d.step, Step::SetProtocol(_) | Step::SetIdle(_) | Step::HubDepth | Step::Running);
        if code == CC_STALL && tolerable {
            d.resume = d.step;
            d.step = Step::Ep0Reset;
            let ctl = TRB_RESET_EP << 10 | (d.slot as u32) << 24 | 1 << 16;
            self.command(i, Cmd::Step, 0, ctl);
            return;
        }
        let why = alloc::format!("control transfer failed, completion code {code}");
        self.fail(i, &why);
    }

    // Enumeration steps.

    fn enable_slot(&mut self, i: usize) {
        let d = self.dev(i);
        if d.parent.is_none() {
            let p = d.root_port as usize - 1;
            let sc = self.portsc(p);
            self.dev(i).speed = ((sc >> 10) & 0xf) as u8;
        }
        let d = self.dev(i);
        d.mps0 = match d.speed {
            speed::HIGH => 64,
            s if s >= speed::SUPER => 512,
            _ => 8,
        };
        d.step = Step::EnableSlot;
        self.command(i, Cmd::Step, 0, TRB_ENABLE_SLOT << 10);
    }

    fn ctx(&self, base: u64, index: usize) -> *mut u32 {
        unsafe { phys::phys_to_virt(base).add(index * self.csz) as *mut u32 }
    }

    /// Fill the EP0 context of the input context.
    fn ep0_context(&self, d: &Device) {
        let ep = self.ctx(d.in_ctx(), 2);
        let ring = d.ep0.base | d.ep0.cycle as u64;
        unsafe {
            write_volatile(ep, 0);
            write_volatile(ep.add(1), 3 << 1 | EP_CONTROL << 3 | (d.mps0 as u32) << 16);
            write_volatile(ep.add(2), ring as u32);
            write_volatile(ep.add(3), (ring >> 32) as u32);
            write_volatile(ep.add(4), 8);
        }
    }

    /// Slot context of the input context with `entries` endpoint
    /// contexts: speed, route, root port, TT and, for a configured hub,
    /// its ports and TT think time.
    fn slot_context(&self, d: &Device, entries: u32) {
        let s = self.ctx(d.in_ctx(), 1);
        let (is_hub, ports, ttt) = match &d.hub {
            Some(h) if h.hub.desc.ports > 0 => {
                let ttt = if d.speed == speed::HIGH { h.hub.desc.tt_think() as u32 } else { 0 };
                (1u32, h.hub.desc.ports as u32, ttt)
            }
            _ => (0, 0, 0),
        };
        unsafe {
            write_volatile(s, d.route & 0xf_ffff | (d.speed as u32) << 20 | is_hub << 26 | entries << 27);
            write_volatile(s.add(1), (d.root_port as u32) << 16 | ports << 24);
            write_volatile(s.add(2), d.tt.0 as u32 | (d.tt.1 as u32) << 8 | ttt << 16);
            write_volatile(s.add(3), 0);
        }
    }

    fn clear_input(&self, d: &Device) {
        unsafe { core::ptr::write_bytes(phys::phys_to_virt(d.in_ctx()), 0, PAGE) };
    }

    fn address_device(&mut self, i: usize, slot: u8) {
        let Some(mem) = alloc_pages(DEV_PAGES, self.limit) else {
            self.fail(i, "out of memory");
            return;
        };
        {
            let d = self.dev(i);
            d.slot = slot;
            d.mem = mem;
            d.ep0 = Ring::new(d.page(3));
        }
        let d = self.devs[i].as_ref().unwrap();
        unsafe { write_volatile((phys::phys_to_virt(self.dcbaa) as *mut u64).add(slot as usize), d.out_ctx()) };
        self.clear_input(d);
        let icc = self.ctx(d.in_ctx(), 0);
        unsafe { write_volatile(icc.add(1), 0b11) };
        self.slot_context(d, 1);
        self.ep0_context(d);
        let ctl = TRB_ADDRESS << 10 | (slot as u32) << 24;
        let in_ctx = d.in_ctx();
        self.dev(i).step = Step::Address;
        self.command(i, Cmd::Step, in_ctx, ctl);
    }

    fn get_device_desc8(&mut self, i: usize) {
        self.dev(i).step = Step::DevDesc8;
        self.control(i, Setup::get_descriptor(usb::DESC_DEVICE, 0, 8));
    }

    fn get_device_desc(&mut self, i: usize) {
        self.dev(i).step = Step::DevDesc;
        self.control(i, Setup::get_descriptor(usb::DESC_DEVICE, 0, 18));
    }

    /// The current control transfer completed: next step.
    fn control_done(&mut self, i: usize) {
        let now = self.now();
        let d = self.dev(i);
        match d.step {
            Step::DevDesc8 => {
                let m = d.data_slice(8)[7];
                let mps = if d.speed >= speed::SUPER { 1u16 << m.min(15) } else { m as u16 };
                if mps != d.mps0 && mps >= 8 {
                    // Tell the controller the real EP0 packet size.
                    d.mps0 = mps;
                    let d = self.devs[i].as_ref().unwrap();
                    self.clear_input(d);
                    unsafe { write_volatile(self.ctx(d.in_ctx(), 0).add(1), 0b10) };
                    self.ep0_context(d);
                    let ctl = TRB_EVALUATE << 10 | (d.slot as u32) << 24;
                    let in_ctx = d.in_ctx();
                    self.dev(i).step = Step::Evaluate;
                    self.command(i, Cmd::Step, in_ctx, ctl);
                } else {
                    self.get_device_desc(i);
                }
            }
            Step::DevDesc => {
                let Some(desc) = usb::parse_device(d.data_slice(18)) else {
                    self.fail(i, "bad device descriptor");
                    return;
                };
                d.desc = desc;
                d.step = Step::ConfigHead;
                self.control(i, Setup::get_descriptor(usb::DESC_CONFIG, 0, 9));
            }
            Step::ConfigHead => {
                let h = d.data_slice(9);
                let total = u16::from_le_bytes([h[2], h[3]]).clamp(9, REPORT_BUF as u16);
                d.step = Step::Config;
                self.control(i, Setup::get_descriptor(usb::DESC_CONFIG, 0, total));
            }
            Step::Config => {
                let (value, ifaces) = usb::parse_interfaces(d.data_slice(REPORT_BUF));
                d.config = value;
                if !self.choose_driver(i, &ifaces) {
                    let name = self.name.clone();
                    let d = self.dev(i);
                    crate::println!(
                        "{name}: port {}: device {:04x}:{:04x} class {:#x} (no driver)",
                        d.path(),
                        d.desc.vendor,
                        d.desc.product,
                        d.desc.class
                    );
                    d.step = Step::Unused;
                    return;
                }
                self.dev(i).step = Step::SetConfig;
                self.control(i, Setup::set_configuration(value));
            }
            Step::SetConfig => {
                if let Some(h) = &d.hub {
                    let usb3 = h.hub.usb3;
                    d.step = Step::HubDesc;
                    self.control(i, Setup::hub_descriptor(usb3));
                } else if d.hids.is_empty() {
                    self.configure_endpoints(i);
                } else {
                    d.step = Step::SetProtocol(0);
                    let iface = d.hids[0].iface.iface;
                    self.control(i, Setup::set_protocol(iface, 0));
                }
            }
            Step::SetProtocol(n) => {
                d.step = Step::SetIdle(n);
                let iface = d.hids[n as usize].iface.iface;
                self.control(i, Setup::set_idle(iface));
            }
            Step::SetIdle(n) => {
                if (n as usize) + 1 < d.hids.len() {
                    d.step = Step::SetProtocol(n + 1);
                    let iface = d.hids[n as usize + 1].iface.iface;
                    self.control(i, Setup::set_protocol(iface, 0));
                } else {
                    self.configure_endpoints(i);
                }
            }
            Step::HubDesc => {
                let Some(desc) = hub::parse_desc(d.data_slice(32)) else {
                    self.fail(i, "bad hub descriptor");
                    return;
                };
                let depth = d.depth;
                let h = d.hub.as_mut().unwrap();
                h.hub.desc = desc;
                if h.hub.usb3 {
                    d.step = Step::HubDepth;
                    self.control(i, Setup::set_hub_depth(depth));
                } else {
                    self.configure_endpoints(i);
                }
            }
            Step::HubDepth => self.configure_endpoints(i),
            Step::Running => {
                d.ctl_busy = false;
                if d.hub.is_some() {
                    let mut buf = [0u8; 32];
                    buf.copy_from_slice(d.data_slice(32));
                    d.hub.as_mut().unwrap().hub.done(&buf, now);
                } else {
                    crate::trace!("usb: slot {} LEDs set to {:#x}", d.slot, d.leds);
                }
            }
            _ => {}
        }
    }

    /// Pick the driver for a configuration: hub, mass storage (Bulk-Only)
    /// or HID boot devices.
    fn choose_driver(&mut self, i: usize, ifaces: &[usb::Interface]) -> bool {
        let name = self.name.clone();
        let d = self.dev(i);
        let alt0 = || ifaces.iter().filter(|f| f.alt == 0);
        if let Some(f) = alt0().find(|f| f.class == usb::CLASS_HUB) {
            let Some(ep) = usb::Endpoint::find(&f.endpoints, usb::EP_INTERRUPT, true) else { return false };
            if d.depth >= MAX_DEPTH {
                crate::println!("{name}: port {}: hub nested too deeply", d.path());
                return false;
            }
            let ring = Ring::new(d.page(4));
            let hub = Hub::new(d.speed >= speed::SUPER);
            d.hub = Some(HubDev { hub, ep, ring, children: [None; hub::MAX_PORTS as usize + 1], errors: 0 });
            return true;
        }
        let msc = alt0().find(|f| f.class == usb::CLASS_STORAGE && f.protocol == usb::MSC_BOT);
        if let Some(f) = msc {
            let ep_in = usb::Endpoint::find(&f.endpoints, usb::EP_BULK, true);
            let ep_out = usb::Endpoint::find(&f.endpoints, usb::EP_BULK, false);
            if let (Some(ep_in), Some(ep_out)) = (ep_in, ep_out) {
                if f.subclass != usb::MSC_SCSI {
                    crate::println!("{name}: port {}: mass storage subclass {:#x}, trying SCSI", d.path(), f.subclass);
                }
                let (ring_in, ring_out) = (Ring::new(d.page(4)), Ring::new(d.page(5)));
                d.msc = Some(Msc { iface: f.number, ep_in, ep_out, ring_in, ring_out });
                return true;
            }
        }
        for (n, f) in usb::hid_boot_interfaces(ifaces).into_iter().take(MAX_HID).enumerate() {
            let Some(kind) = hid::Kind::from_protocol(f.protocol) else { continue };
            let ring = Ring::new(d.page(4 + n));
            let dci = (f.endpoint & 0xf) * 2 + 1;
            let len = f.max_packet.clamp(1, REPORT_MAX as u16);
            d.hids.push(Hid { iface: f, kind, dci, ring, len, handle: 0, errors: 0 });
        }
        !d.hids.is_empty()
    }

    /// xHCI endpoint interval (2^n * 125 us) for an interrupt endpoint.
    fn interval(speed: u8, b_interval: u8) -> u32 {
        if speed == usb::speed::FULL || speed == usb::speed::LOW {
            // bInterval in frames (ms): log2(ms * 8).
            let us125 = (b_interval.max(1) as u32) * 8;
            (31 - us125.leading_zeros()).clamp(3, 10)
        } else {
            (b_interval.clamp(1, 16) - 1) as u32
        }
    }

    /// Fill endpoint context `dci` of the input context.
    fn ep_context(&self, d: &Device, ep: &usb::Endpoint, kind: u32, ring: u64) {
        let ctx = self.ctx(d.in_ctx(), 1 + ep.dci() as usize);
        let mps = ep.max_packet as u32;
        let burst = ep.max_burst as u32;
        let (interval, esit, avg) = if kind == EP_INTERRUPT_IN {
            (Self::interval(d.speed, ep.interval), mps * (burst + 1), mps)
        } else {
            (0, 0, 3072)
        };
        let ring = ring | 1;
        unsafe {
            write_volatile(ctx, interval << 16 | (esit >> 16) << 24);
            write_volatile(ctx.add(1), 3 << 1 | kind << 3 | burst << 8 | mps << 16);
            write_volatile(ctx.add(2), ring as u32);
            write_volatile(ctx.add(3), (ring >> 32) as u32);
            write_volatile(ctx.add(4), (esit & 0xffff) << 16 | avg);
        }
    }

    fn configure_endpoints(&mut self, i: usize) {
        let d = self.devs[i].as_ref().unwrap();
        self.clear_input(d);
        let mut eps: Vec<(usb::Endpoint, u32, u64)> = Vec::new();
        for h in &d.hids {
            let ep = usb::Endpoint { address: h.iface.endpoint, attributes: 3, max_packet: h.iface.max_packet, interval: h.iface.interval, max_burst: 0 };
            eps.push((ep, EP_INTERRUPT_IN, h.ring.base));
        }
        if let Some(h) = &d.hub {
            eps.push((h.ep, EP_INTERRUPT_IN, h.ring.base));
        }
        if let Some(m) = &d.msc {
            eps.push((m.ep_in, EP_BULK_IN, m.ring_in.base));
            eps.push((m.ep_out, EP_BULK_OUT, m.ring_out.base));
        }
        let mut add = 1u32;
        let mut max_dci = 1;
        for (ep, kind, ring) in &eps {
            add |= 1 << ep.dci();
            max_dci = max_dci.max(ep.dci() as u32);
            self.ep_context(d, ep, *kind, *ring);
        }
        unsafe { write_volatile(self.ctx(d.in_ctx(), 0).add(1), add) };
        self.slot_context(d, max_dci);
        let ctl = TRB_CONFIGURE << 10 | (d.slot as u32) << 24;
        let in_ctx = d.in_ctx();
        self.dev(i).step = Step::Configure;
        self.command(i, Cmd::Step, in_ctx, ctl);
    }

    fn start_running(&mut self, i: usize) {
        let name = self.name.clone();
        let (booting, index) = (self.booting, self.index);
        let d = self.dev(i);
        d.step = Step::Running;
        let (path, vendor, product, slot) = (d.path(), d.desc.vendor, d.desc.product, d.slot);
        if let Some(h) = d.hub.as_mut() {
            h.hub.start();
            let (ports, usb3, dci) = (h.hub.desc.ports, h.hub.usb3, h.ep.dci());
            let tt = if d.speed == speed::HIGH { ", TT" } else { "" };
            let kind = if usb3 { "USB 3 hub" } else { "USB 2 hub" };
            crate::println!("{name}: port {path}: {kind} {vendor:04x}:{product:04x}, {ports} ports{tt} (slot {slot})");
            self.queue_interrupt(i, dci);
            return;
        }
        if d.msc.is_some() {
            crate::println!("{name}: port {path}: USB mass storage {vendor:04x}:{product:04x} (slot {slot})");
            if booting {
                self.new_storage.push(i);
            } else {
                // Set up and mounted by the hot-plug kernel thread.
                let h = Handle { ctrl: index, dev: i, epoch: d.epoch };
                STORAGE_EVENTS.post(StorageEvent::Added(h));
            }
            return;
        }
        // Force the first LED update.
        d.leds = 0xff;
        for h in d.hids.iter_mut() {
            h.handle = hid::attach(h.kind);
            crate::println!(
                "{name}: port {path}: USB {:?} {vendor:04x}:{product:04x} (slot {slot}, endpoint {:#x})",
                h.kind,
                h.iface.endpoint
            );
        }
        let dcis: Vec<u8> = d.hids.iter().map(|h| h.dci).collect();
        for dci in dcis {
            self.queue_interrupt(i, dci);
        }
    }

    /// Queue the next interrupt-IN transfer of endpoint `dci` (a HID
    /// interface or a hub's status-change endpoint).
    fn queue_interrupt(&mut self, i: usize, dci: u8) {
        let Some(d) = self.devs[i].as_mut() else { return };
        let (data, slot) = (d.data(), d.slot);
        if let Some(n) = d.hids.iter().position(|h| h.dci == dci) {
            let h = &mut d.hids[n];
            let buf = data + (REPORT_BUF + n * REPORT_MAX) as u64;
            h.ring.push(buf, h.len as u32, TRB_NORMAL << 10 | TRB_IOC | TRB_ISP);
        } else if let Some(h) = d.hub.as_mut().filter(|h| h.ep.dci() == dci) {
            let len = h.ep.max_packet.clamp(1, 8) as u32;
            h.ring.push(data + HUB_BUF as u64, len, TRB_NORMAL << 10 | TRB_IOC | TRB_ISP);
        } else {
            return;
        }
        self.ring_doorbell(slot, dci);
    }

    /// Keep keyboard LEDs in step with the console lock state.
    fn leds(&mut self) {
        let want = hid::leds();
        for i in 0..self.devs.len() {
            let Some(d) = self.devs[i].as_mut() else { continue };
            if d.step != Step::Running || d.ctl_busy || d.leds == want {
                continue;
            }
            let Some(h) = d.hids.iter().find(|h| h.kind == hid::Kind::Keyboard) else { continue };
            let iface = h.iface.iface;
            d.leds = want;
            d.ctl_busy = true;
            unsafe { write_volatile(phys::phys_to_virt(d.data()), want) };
            self.control(i, Setup::set_leds(iface));
        }
    }

    // Thread-context transfers.

    /// A transfer event for a thread-context transfer (or a stray one for
    /// a bulk endpoint): record it. Returns whether it was one.
    fn sync_event(&mut self, i: usize, dci: u8, ev: &Event) -> bool {
        let d = self.dev(i);
        let bulk = d.is_bulk(dci);
        let Some(s) = d.sync.as_mut().filter(|s| s.dci == dci) else { return bulk };
        let Some(k) = s.trbs.iter().position(|t| t.0 == ev.param) else { return bulk };
        if s.done.is_some() {
            return true;
        }
        let code = ev.code();
        let before: usize = s.trbs[..k].iter().map(|t| t.1 as usize).sum();
        let total: usize = s.trbs.iter().map(|t| t.1 as usize).sum();
        let last = k + 1 == s.trbs.len();
        match code {
            CC_SUCCESS | CC_SHORT => {
                if code == CC_SHORT {
                    let residue = (ev.status & 0xff_ffff).min(s.trbs[k].1);
                    s.short = Some(before + (s.trbs[k].1 - residue) as usize);
                }
                // A short packet ends a bulk TD; a control transfer still
                // completes its status stage.
                if last || (code == CC_SHORT && dci != 1) {
                    s.done = Some((CC_SUCCESS, s.short.unwrap_or(total)));
                }
            }
            _ => s.done = Some((code, before)),
        }
        true
    }

    fn alive(&self, h: Handle) -> bool {
        self.devs.get(h.dev).and_then(|d| d.as_ref()).is_some_and(|d| d.epoch == h.epoch && d.step == Step::Running)
    }
}

/// A configured device, as thread-context callers name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    ctrl: usize,
    dev: usize,
    epoch: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XferError {
    /// The endpoint stalled (and is halted).
    Stall,
    /// The device was removed.
    Gone,
    Timeout,
    /// Another completion code.
    Failed(u8),
}

/// What the mass storage driver needs about a Bulk-Only interface.
pub struct StorageDev {
    pub handle: Handle,
    pub iface: u8,
    pub ep_in: usb::Endpoint,
    pub ep_out: usb::Endpoint,
    pub vendor: u16,
    pub product: u16,
    /// "xhci 00:03.0 port 1.2", for messages.
    pub name: String,
}

/// Mass storage devices that came or went after boot.
pub enum StorageEvent {
    Added(Handle),
    Removed(Handle),
}

pub static STORAGE_EVENTS: crate::task::kthread::Inbox<StorageEvent> = crate::task::kthread::Inbox::new();

/// The Bulk-Only interface of a hot-plugged device, if it is still there.
pub fn hotplug_storage(h: Handle) -> Option<StorageDev> {
    let s = storage_info(h.ctrl, h.dev)?;
    (s.handle == h).then_some(s)
}

fn storage_info(ctrl: usize, dev: usize) -> Option<StorageDev> {
    let cs = CTRLS.lock();
    let c = cs.get(ctrl)?;
    let d = c.devs.get(dev)?.as_ref()?;
    let m = d.msc.as_ref()?;
    Some(StorageDev {
        handle: Handle { ctrl, dev, epoch: d.epoch },
        iface: m.iface,
        ep_in: m.ep_in,
        ep_out: m.ep_out,
        vendor: d.desc.vendor,
        product: d.desc.product,
        name: alloc::format!("{} port {}", c.name, d.path()),
    })
}

/// Run `f` on the controller and the live device of `h`.
fn with_dev<T>(h: Handle, f: impl FnOnce(&mut Ctrl) -> T) -> Result<T, XferError> {
    let mut cs = CTRLS.lock();
    let c = cs.get_mut(h.ctrl).ok_or(XferError::Gone)?;
    if !c.alive(h) {
        return Err(XferError::Gone);
    }
    Ok(f(c))
}

/// Poll the controller until `check` returns something, sleeping until
/// the next interrupt between polls (yielding the CPU without one).
fn wait<T>(h: Handle, timeout_ms: u64, mut check: impl FnMut(&mut Ctrl) -> Option<T>) -> Result<T, XferError> {
    let mut deadline = None;
    loop {
        let (irq, seen, left) = {
            let mut cs = CTRLS.lock();
            let c = cs.get_mut(h.ctrl).ok_or(XferError::Gone)?;
            // Interrupts after this point are seen by the wait below.
            let seen = c.irq.map_or(0, |e| e.signals());
            c.poll_once(false);
            if !c.alive(h) {
                return Err(XferError::Gone);
            }
            if let Some(v) = check(c) {
                return Ok(v);
            }
            let now = c.now();
            let end = *deadline.get_or_insert(now + timeout_ms);
            if now >= end {
                return Err(XferError::Timeout);
            }
            (c.irq, seen, end - now)
        };
        match irq {
            // Re-poll at least every 20 ms (a lost interrupt).
            Some(e) => {
                e.wait_signal(seen, left.min(20));
            }
            None => crate::task::sched::yield_now(),
        }
    }
}

/// Wait about `ms` milliseconds (also during boot, with the timer off).
pub fn delay(h: Handle, ms: u64) {
    let _ = wait(h, ms, |_| None::<()>);
}

/// Push a control transfer (setup, optional data, status) for buffer
/// `data`; returns the TRB addresses and lengths and the status TRB.
fn push_control(ring: &mut Ring, s: Setup, data: u64) -> (Vec<(u64, u32)>, u64) {
    let len = s.length as u32;
    let trt = if len == 0 { 0 } else if s.is_in() { 3 } else { 2 };
    let mut trbs = Vec::new();
    trbs.push((ring.push(u64::from_le_bytes(s.bytes()), 8, TRB_SETUP << 10 | TRB_IDT | trt << 16), 0));
    if len > 0 {
        let dir = if s.is_in() { TRB_DIR_IN } else { 0 };
        trbs.push((ring.push(data, len, TRB_DATA << 10 | TRB_ISP | dir), len));
    }
    let status_in = if len > 0 && s.is_in() { 0 } else { TRB_DIR_IN };
    let status = ring.push(0, 0, TRB_STATUS << 10 | TRB_IOC | status_in);
    trbs.push((status, 0));
    (trbs, status)
}

/// Wait for the transfer on `dci` and turn its completion into a result.
fn finish(h: Handle, dci: u8, timeout_ms: u64) -> Result<usize, XferError> {
    let r = wait(h, timeout_ms, |c| c.devs[h.dev].as_ref()?.sync.as_ref()?.done);
    let _ = with_dev(h, |c| c.dev(h.dev).sync = None);
    match r {
        Ok((CC_SUCCESS, n)) => Ok(n),
        Ok((CC_STALL, _)) => Err(XferError::Stall),
        Ok((code, _)) => Err(XferError::Failed(code)),
        Err(XferError::Timeout) => {
            let _ = stop_endpoint(h, dci);
            Err(XferError::Timeout)
        }
        Err(e) => Err(e),
    }
}

/// A control transfer on EP0 from thread context, with `data` (physical,
/// `s.length` bytes) as buffer. Returns the bytes transferred.
pub fn control(h: Handle, s: Setup, data: u64, timeout_ms: u64) -> Result<usize, XferError> {
    with_dev(h, |c| {
        let d = c.dev(h.dev);
        let (trbs, _) = push_control(&mut d.ep0, s, data);
        d.sync = Some(SyncXfer { dci: 1, trbs, short: None, done: None });
        let slot = d.slot;
        c.ring_doorbell(slot, 1);
    })?;
    finish(h, 1, timeout_ms)
}

/// Packets still to come after this TRB, for the TD Size field.
fn td_size(remaining: usize, mps: usize) -> u32 {
    (remaining.div_ceil(mps.max(1)) as u32).min(31)
}

/// A bulk transfer of `len` bytes from or to the physically contiguous
/// buffer `buf` on endpoint `ep`. Returns the bytes transferred.
pub fn bulk(h: Handle, ep: &usb::Endpoint, buf: u64, len: usize, timeout_ms: u64) -> Result<usize, XferError> {
    let dci = ep.dci();
    let mps = ep.max_packet as usize;
    with_dev(h, |c| {
        let d = c.dev(h.dev);
        let slot = d.slot;
        let Some(ring) = d.ring(dci) else { return Err(XferError::Failed(0)) };
        // One TRB per page: no TRB crosses a 64 KB boundary.
        let mut trbs = Vec::new();
        let mut off = 0;
        loop {
            let addr = buf + off as u64;
            let n = (len - off).min(PAGE - (addr as usize & (PAGE - 1)));
            let last = off + n >= len;
            let flags = if last { TRB_IOC } else { TRB_CH };
            let status = n as u32 | td_size(len - off - n, mps) << 17;
            trbs.push((ring.push(addr, status, TRB_NORMAL << 10 | TRB_ISP | flags), n as u32));
            off += n;
            if last {
                break;
            }
        }
        d.sync = Some(SyncXfer { dci, trbs, short: None, done: None });
        c.ring_doorbell(slot, dci);
        Ok(())
    })??;
    finish(h, dci, timeout_ms)
}

/// Issue a command from thread context; returns its completion code.
fn command_sync(h: Handle, param: u64, control: u32) -> Result<u8, XferError> {
    let trb = with_dev(h, |c| c.command(h.dev, Cmd::Sync, param, control))?;
    let found = |c: &mut Ctrl| {
        let n = c.sync_cmds.iter().position(|x| x.0 == trb)?;
        Some(c.sync_cmds.remove(n).1)
    };
    let r = wait(h, STEP_TIMEOUT_MS, found);
    if r.is_err() {
        // Do not let a late completion pile up.
        let _ = with_dev(h, |c| c.sync_cmds.clear());
    }
    r
}

/// Point endpoint `dci` (stopped or halted) past everything queued.
fn set_dequeue(h: Handle, dci: u8) -> Result<(), XferError> {
    let (slot, deq) = with_dev(h, |c| {
        let d = c.dev(h.dev);
        (d.slot, d.ring(dci).map_or(0, |r| r.enqueue_ptr()))
    })?;
    let ctl = TRB_SET_DEQ << 10 | (slot as u32) << 24 | (dci as u32) << 16;
    match command_sync(h, deq, ctl)? {
        CC_SUCCESS => Ok(()),
        code => Err(XferError::Failed(code)),
    }
}

/// Recover endpoint `dci` after a stall or error on the controller side
/// (Reset Endpoint resets its data toggle), dropping what was queued.
/// The device's own halt is cleared with CLEAR_FEATURE by the caller.
pub fn clear_halt(h: Handle, dci: u8) -> Result<(), XferError> {
    let slot = with_dev(h, |c| c.dev(h.dev).slot)?;
    // Fails with a context state error if the endpoint is not halted.
    let _ = command_sync(h, 0, TRB_RESET_EP << 10 | (slot as u32) << 24 | (dci as u32) << 16)?;
    set_dequeue(h, dci)
}

/// Abort what is queued on endpoint `dci` (after a timeout).
fn stop_endpoint(h: Handle, dci: u8) -> Result<(), XferError> {
    let slot = with_dev(h, |c| c.dev(h.dev).slot)?;
    let _ = command_sync(h, 0, TRB_STOP_EP << 10 | (slot as u32) << 24 | (dci as u32) << 16)?;
    set_dequeue(h, dci)
}

fn free_pages(mem: u64) {
    for i in 0..DEV_PAGES {
        phys::free(mem + (i * PAGE) as u64);
    }
}
