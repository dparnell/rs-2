//! xHCI (USB 3) host controllers: root ports, device enumeration and the
//! interrupt-IN transfers of HID boot keyboards and mice.
//!
//! Bring-up follows the xHCI specification: BIOS hand-off through the USB
//! Legacy Support capability, halt and reset, scratchpad buffers, the
//! device context base address array, a command ring and one event ring
//! (interrupter 0, one segment). Interrupts stay off: the event ring and
//! the port status registers are polled, at boot in a loop until the
//! devices present are configured, afterwards from the timer tick.
//!
//! Everything is asynchronous and event driven so that the same code runs
//! at boot and in the tick: each root port holds at most one device whose
//! enumeration step advances when its command or control transfer
//! completes. Hubs are not supported; devices are configured only if they
//! have HID boot interfaces.

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::hid;
use super::pci;
use super::usb::{self, speed, Setup};
use crate::mm::{mmio, phys};
use crate::sync::SpinLock;

const PAGE: usize = 4096;
/// TRBs per ring segment (one page); the last one is the link TRB.
const TRBS: usize = 256;
/// Pages per device: input context, output context, data buffer, EP0
/// ring, interrupt endpoint rings.
const DEV_PAGES: usize = 4 + MAX_HID;
/// HID interfaces used per device.
const MAX_HID: usize = 4;
/// Offset of the interrupt transfer buffers in the data page.
const REPORT_BUF: usize = 2048;
const REPORT_MAX: usize = 64;
/// Time allowed for one command or control transfer.
const STEP_TIMEOUT_MS: u64 = 1000;
/// Boot-time wait for devices to appear and be configured.
const BOOT_MIN_MS: u64 = 250;
const BOOT_MAX_MS: u64 = 3000;
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
const TRB_SET_DEQ: u32 = 16;
const EV_TRANSFER: u32 = 32;
const EV_COMMAND: u32 = 33;
const EV_PORT: u32 = 34;
const EV_HOST: u32 = 37;

// TRB control bits.
const TRB_CYCLE: u32 = 1 << 0;
const TRB_TC: u32 = 1 << 1;
const TRB_ISP: u32 = 1 << 2;
const TRB_IOC: u32 = 1 << 5;
const TRB_IDT: u32 = 1 << 6;
const TRB_DIR_IN: u32 = 1 << 16;

// Completion codes.
const CC_SUCCESS: u8 = 1;
const CC_STALL: u8 = 6;
const CC_SHORT: u8 = 13;

// Endpoint types (endpoint context).
const EP_CONTROL: u32 = 4;
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
            // Hand the link TRB over and wrap.
            let link = unsafe { phys::phys_to_virt(self.base).add((TRBS - 1) * 16) as *mut u32 };
            unsafe { write_volatile(link.add(3), TRB_LINK << 10 | TRB_TC | cycle) };
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
    /// Connect debounce before the port reset.
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

struct Device {
    /// Distinguishes this device from a later one on the same port.
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
    hids: Vec<Hid>,
    /// LED state last sent; a SET_REPORT is in flight.
    leds: u8,
    ctl_busy: bool,
    reports: u32,
}

impl Device {
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
}

/// Commands in flight, with what they were for.
#[derive(Clone, Copy)]
enum Cmd {
    /// Part of the device's enumeration (its current step).
    Step,
    /// Interrupt endpoint recovery after an error.
    EpReset(u8),
    EpDequeue(u8),
    /// Slot of a removed device; its memory is freed on completion.
    DisableSlot(u64),
}

struct Ctrl {
    name: alloc::string::String,
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
    devs: Vec<Option<Device>>,
    next_epoch: u32,
    clock: Clock,
    last_scan: u64,
    dead: bool,
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

/// Initialise the controller and configure the HID devices present.
pub fn probe(dev: &pci::Device) {
    dev.enable();
    // Polled: mask legacy INTx.
    dev.write(0x04, (dev.read(0x04) & 0xffff) | 1 << 10);
    let bar = dev.bar(0);
    if bar == 0 {
        return;
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
            return;
        }
    }
    wr32(op, USBCMD, CMD_HCRST);
    // Some controllers must not be touched right after the reset.
    clock.wait(2, || false);
    if !clock.wait(1000, || rd32(op, USBCMD) & CMD_HCRST == 0 && rd32(op, USBSTS) & STS_CNR == 0) {
        crate::println!("{name}: reset timed out");
        return;
    }
    let page_bits = rd32(op, PAGESIZE) & 0xffff;
    let hc_page = PAGE << page_bits.trailing_zeros().min(15);
    let scratch = (((hcs2 >> 21) & 0x1f) << 5 | (hcs2 >> 27) & 0x1f) as usize;

    // DCBAA, command ring, event ring, ERST.
    let Some(mem) = alloc_pages(4, limit) else { return };
    let dcbaa = mem;
    let cmd_ring = mem + PAGE as u64;
    let ev_base = mem + 2 * PAGE as u64;
    let erst = mem + 3 * PAGE as u64;
    if scratch > 0 {
        let per = hc_page / PAGE;
        let arr_pages = (scratch * 8).div_ceil(PAGE);
        // Over-allocate to align the buffers to the controller page size.
        let Some(arr) = alloc_pages(arr_pages, limit) else { return };
        let Some(bufs) = alloc_pages(scratch * per + per - 1, limit) else { return };
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
        return;
    }
    crate::println!("{name}: {ports} ports, {slots} slots, {csz}-byte contexts, {scratch} scratchpads");

    let mut c = Ctrl {
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
        devs: (0..ports).map(|_| None).collect(),
        next_epoch: 1,
        clock,
        last_scan: 0,
        dead: false,
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
    c.boot_enumerate();
    CTRLS.lock().push(c);
}

/// Poll every controller; called from the timer interrupt.
pub fn tick() {
    if crate::irq::ticks() % POLL_TICKS != 0 {
        return;
    }
    let Some(mut cs) = CTRLS.try_lock() else { return };
    if cs.is_empty() {
        return;
    }
    for c in cs.iter_mut() {
        c.poll();
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

    /// Poll at boot until the connected devices are configured.
    fn boot_enumerate(&mut self) {
        let start = self.now();
        loop {
            self.poll_once(true);
            let t = self.now() - start;
            let busy = self.devs.iter().flatten().any(|d| {
                !matches!(d.step, Step::Running | Step::Unused | Step::Failed)
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
        self.timers(now);
        self.leds();
    }

    /// Notice connects and disconnects on the root ports.
    fn scan_ports(&mut self, now: u64) {
        for p in 0..self.ports {
            let sc = self.portsc(p);
            if sc & PORT_CSC != 0 {
                self.port_write(p, PORT_CSC);
                if self.devs[p].is_some() {
                    self.remove(p);
                }
            }
            let connected = sc & PORT_CCS != 0;
            if !connected && self.devs[p].is_some() {
                self.remove(p);
            }
            if connected && self.devs[p].is_none() {
                self.connect(p, now);
            }
        }
    }

    fn connect(&mut self, p: usize, now: u64) {
        let epoch = self.next_epoch;
        self.next_epoch += 1;
        self.devs[p] = Some(Device {
            epoch,
            speed: 0,
            slot: 0,
            step: Step::Debounce(now + 100),
            resume: Step::Failed,
            deadline: 0,
            mem: 0,
            ep0: Ring { base: 0, idx: 0, cycle: true },
            mps0: 8,
            status_trb: 0,
            desc: usb::DeviceDesc::default(),
            config: 0,
            hids: Vec::new(),
            leds: 0,
            ctl_busy: false,
            reports: 0,
        });
    }

    /// Forget the device on port `p` and release its slot.
    fn remove(&mut self, p: usize) {
        let Some(d) = self.devs[p].take() else { return };
        for h in &d.hids {
            hid::detach(h.handle);
        }
        if matches!(d.step, Step::Running) {
            crate::println!("{}: port {} device removed", self.name, p + 1);
        }
        if d.slot != 0 {
            let trb = self.cmd.push(0, 0, TRB_DISABLE_SLOT << 10 | (d.slot as u32) << 24);
            self.cmds.push((trb, p, 0, Cmd::DisableSlot(d.mem)));
            self.ring_doorbell(0, 0);
        } else if d.mem != 0 {
            free_pages(d.mem);
        }
    }

    fn fail(&mut self, p: usize, why: &str) {
        let name = self.name.clone();
        if let Some(d) = self.devs[p].as_mut() {
            crate::println!("{name}: port {}: {why} (step {:?})", p + 1, d.step);
            d.step = Step::Failed;
        }
    }

    /// Deadlines: debounce, reset completion, recovery, timeouts.
    fn timers(&mut self, now: u64) {
        for p in 0..self.ports {
            let Some(d) = self.devs[p].as_ref() else { continue };
            let (step, deadline) = (d.step, d.deadline);
            match step {
                Step::Debounce(t) if now >= t => {
                    let sc = self.portsc(p);
                    let d = self.devs[p].as_mut().unwrap();
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
                            self.devs[p].as_mut().unwrap().step = Step::Settle(now + 10);
                        } else {
                            self.fail(p, "port not enabled after reset");
                        }
                    } else if now >= deadline {
                        self.fail(p, "port reset timed out");
                    }
                }
                Step::Settle(t) if now >= t => self.enable_slot(p),
                Step::Debounce(_) | Step::Settle(_) | Step::Running | Step::Unused | Step::Failed => {}
                _ => {
                    if now >= deadline {
                        self.fail(p, "timed out");
                    }
                }
            }
        }
    }

    fn command(&mut self, p: usize, kind: Cmd, param: u64, control: u32) {
        let epoch = self.devs[p].as_ref().map_or(0, |d| d.epoch);
        let trb = self.cmd.push(param, 0, control);
        self.cmds.push((trb, p, epoch, kind));
        let now = self.now();
        if let Some(d) = self.devs[p].as_mut() {
            if matches!(kind, Cmd::Step) {
                d.deadline = now + STEP_TIMEOUT_MS;
            }
        }
        self.ring_doorbell(0, 0);
    }

    /// Start a control transfer on EP0; completion advances the step.
    fn control(&mut self, p: usize, s: Setup) {
        let now = self.now();
        let Some(d) = self.devs[p].as_mut() else { return };
        let len = s.length as usize;
        let data = d.data();
        let trt = if len == 0 { 0 } else if s.is_in() { 3 } else { 2 };
        if s.is_in() {
            unsafe { core::ptr::write_bytes(phys::phys_to_virt(data), 0, len.min(REPORT_BUF)) };
        }
        d.ep0.push(u64::from_le_bytes(s.bytes()), 8, TRB_SETUP << 10 | TRB_IDT | trt << 16);
        if len > 0 {
            let dir = if s.is_in() { TRB_DIR_IN } else { 0 };
            d.ep0.push(data, len as u32, TRB_DATA << 10 | dir);
        }
        let status_in = if len > 0 && s.is_in() { 0 } else { TRB_DIR_IN };
        d.status_trb = d.ep0.push(0, 0, TRB_STATUS << 10 | TRB_IOC | status_in);
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
        if any {
            // Update the dequeue pointer, clearing Event Handler Busy.
            wr64(self.rt, ERDP, self.ev_base + (self.ev_idx * 16) as u64 | 1 << 3);
        }
    }

    fn event(&mut self, ev: Event) {
        match ev.kind() {
            EV_COMMAND => self.command_done(ev),
            EV_TRANSFER => self.transfer_done(ev),
            EV_PORT => {} // the port registers are scanned
            EV_HOST => crate::println!("{}: host controller event {}", self.name, ev.code()),
            _ => {}
        }
    }

    fn command_done(&mut self, ev: Event) {
        let Some(i) = self.cmds.iter().position(|c| c.0 == ev.param) else { return };
        let (_, p, epoch, kind) = self.cmds.remove(i);
        if let Cmd::DisableSlot(mem) = kind {
            if ev.slot() != 0 {
                unsafe { write_volatile((phys::phys_to_virt(self.dcbaa) as *mut u64).add(ev.slot() as usize), 0) };
            }
            if mem != 0 {
                free_pages(mem);
            }
            return;
        }
        if !self.devs[p].as_ref().is_some_and(|d| d.epoch == epoch) {
            return; // the device is gone
        }
        match kind {
            Cmd::Step => self.step_command_done(p, ev),
            Cmd::EpReset(i) => {
                let d = self.devs[p].as_ref().unwrap();
                let Some(h) = d.hids.get(i as usize) else { return };
                let (slot, dci, deq) = (d.slot, h.dci, h.ring.enqueue_ptr());
                let ctl = TRB_SET_DEQ << 10 | (slot as u32) << 24 | (dci as u32) << 16;
                self.command(p, Cmd::EpDequeue(i), deq, ctl);
            }
            Cmd::EpDequeue(i) => self.queue_report(p, i as usize),
            Cmd::DisableSlot(_) => {}
        }
    }

    fn step_command_done(&mut self, p: usize, ev: Event) {
        let step = self.devs[p].as_ref().unwrap().step;
        if ev.code() != CC_SUCCESS {
            let why = alloc::format!("command failed, completion code {}", ev.code());
            self.fail(p, &why);
            return;
        }
        match step {
            Step::EnableSlot => self.address_device(p, ev.slot()),
            Step::Address => self.get_device_desc8(p),
            Step::Evaluate => self.get_device_desc(p),
            Step::Configure => self.start_running(p),
            Step::Ep0Reset => {
                let d = self.devs[p].as_ref().unwrap();
                let ctl = TRB_SET_DEQ << 10 | (d.slot as u32) << 24 | 1 << 16;
                let deq = d.ep0.enqueue_ptr();
                self.devs[p].as_mut().unwrap().step = Step::Ep0Dequeue;
                self.command(p, Cmd::Step, deq, ctl);
            }
            Step::Ep0Dequeue => {
                let d = self.devs[p].as_mut().unwrap();
                d.step = d.resume;
                if d.step == Step::Running {
                    // A stalled LED report: the keyboard has no LEDs.
                    d.ctl_busy = false;
                } else {
                    // A stalled optional request: carry on.
                    self.control_done(p);
                }
            }
            _ => {}
        }
    }

    fn transfer_done(&mut self, ev: Event) {
        let slot = ev.slot();
        let Some(p) = self.devs.iter().position(|d| d.as_ref().is_some_and(|d| d.slot == slot && slot != 0)) else {
            return;
        };
        let dci = ev.endpoint();
        if dci == 1 {
            let d = self.devs[p].as_ref().unwrap();
            let code = ev.code();
            if code == CC_SUCCESS && ev.param == d.status_trb {
                self.control_done(p);
            } else if code != CC_SUCCESS && code != CC_SHORT {
                self.control_error(p, code);
            }
            return;
        }
        let d = self.devs[p].as_mut().unwrap();
        let Some(i) = d.hids.iter().position(|h| h.dci == dci) else { return };
        let code = ev.code();
        if code == CC_SUCCESS || code == CC_SHORT {
            let data = d.data();
            let h = &mut d.hids[i];
            h.errors = 0;
            let got = (h.len as usize).saturating_sub((ev.status & 0xff_ffff) as usize);
            let off = REPORT_BUF + i * REPORT_MAX;
            let mut report = [0u8; REPORT_MAX];
            let src = unsafe { core::slice::from_raw_parts(phys::phys_to_virt(data).add(off), got) };
            report[..got].copy_from_slice(src);
            let (handle, kind) = (h.handle, h.kind);
            if d.reports < TRACE_REPORTS {
                d.reports += 1;
                crate::trace!("usb: slot {slot} {kind:?} report {:02x?}", &report[..got]);
            }
            self.queue_report(p, i);
            hid::report(handle, &report[..got]);
        } else {
            let h = &mut d.hids[i];
            h.errors += 1;
            let slot = d.slot;
            let dci = h.dci;
            if h.errors > MAX_EP_ERRORS {
                self.fail(p, "too many interrupt endpoint errors");
                return;
            }
            // The endpoint halted: reset it and skip the failed TD.
            let ctl = TRB_RESET_EP << 10 | (slot as u32) << 24 | (dci as u32) << 16;
            self.command(p, Cmd::EpReset(i as u8), 0, ctl);
        }
    }

    /// EP0 failed. Optional HID requests may stall; anything else fails
    /// the device.
    fn control_error(&mut self, p: usize, code: u8) {
        let d = self.devs[p].as_mut().unwrap();
        let tolerable = matches!(d.step, Step::SetProtocol(_) | Step::SetIdle(_) | Step::Running);
        if code == CC_STALL && tolerable {
            d.resume = d.step;
            d.step = Step::Ep0Reset;
            let ctl = TRB_RESET_EP << 10 | (d.slot as u32) << 24 | 1 << 16;
            self.command(p, Cmd::Step, 0, ctl);
            return;
        }
        let why = alloc::format!("control transfer failed, completion code {code}");
        self.fail(p, &why);
    }

    // Enumeration steps.

    fn enable_slot(&mut self, p: usize) {
        let sc = self.portsc(p);
        let d = self.devs[p].as_mut().unwrap();
        d.speed = ((sc >> 10) & 0xf) as u8;
        d.mps0 = match d.speed {
            speed::HIGH => 64,
            s if s >= speed::SUPER => 512,
            _ => 8,
        };
        d.step = Step::EnableSlot;
        self.command(p, Cmd::Step, 0, TRB_ENABLE_SLOT << 10);
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

    /// Slot context of the input context with `entries` endpoint contexts.
    fn slot_context(&self, d: &Device, p: usize, entries: u32) {
        let s = self.ctx(d.in_ctx(), 1);
        unsafe {
            write_volatile(s, (d.speed as u32) << 20 | entries << 27);
            write_volatile(s.add(1), ((p + 1) as u32) << 16);
            write_volatile(s.add(2), 0);
            write_volatile(s.add(3), 0);
        }
    }

    fn clear_input(&self, d: &Device) {
        unsafe { core::ptr::write_bytes(phys::phys_to_virt(d.in_ctx()), 0, PAGE) };
    }

    fn address_device(&mut self, p: usize, slot: u8) {
        let Some(mem) = alloc_pages(DEV_PAGES, self.limit) else {
            self.fail(p, "out of memory");
            return;
        };
        {
            let d = self.devs[p].as_mut().unwrap();
            d.slot = slot;
            d.mem = mem;
            d.ep0 = Ring::new(d.page(3));
        }
        let d = self.devs[p].as_ref().unwrap();
        unsafe { write_volatile((phys::phys_to_virt(self.dcbaa) as *mut u64).add(slot as usize), d.out_ctx()) };
        self.clear_input(d);
        let icc = self.ctx(d.in_ctx(), 0);
        unsafe { write_volatile(icc.add(1), 0b11) };
        self.slot_context(d, p, 1);
        self.ep0_context(d);
        let ctl = TRB_ADDRESS << 10 | (slot as u32) << 24;
        let in_ctx = d.in_ctx();
        self.devs[p].as_mut().unwrap().step = Step::Address;
        self.command(p, Cmd::Step, in_ctx, ctl);
    }

    fn get_device_desc8(&mut self, p: usize) {
        self.devs[p].as_mut().unwrap().step = Step::DevDesc8;
        self.control(p, Setup::get_descriptor(usb::DESC_DEVICE, 0, 8));
    }

    fn get_device_desc(&mut self, p: usize) {
        self.devs[p].as_mut().unwrap().step = Step::DevDesc;
        self.control(p, Setup::get_descriptor(usb::DESC_DEVICE, 0, 18));
    }

    /// The current control transfer completed: next step.
    fn control_done(&mut self, p: usize) {
        let d = self.devs[p].as_mut().unwrap();
        match d.step {
            Step::DevDesc8 => {
                let m = d.data_slice(8)[7];
                let mps = if d.speed >= speed::SUPER { 1u16 << m.min(15) } else { m as u16 };
                if mps != d.mps0 && mps >= 8 {
                    // Tell the controller the real EP0 packet size.
                    d.mps0 = mps;
                    let d = self.devs[p].as_ref().unwrap();
                    self.clear_input(d);
                    unsafe { write_volatile(self.ctx(d.in_ctx(), 0).add(1), 0b10) };
                    self.ep0_context(d);
                    let ctl = TRB_EVALUATE << 10 | (d.slot as u32) << 24;
                    let in_ctx = d.in_ctx();
                    self.devs[p].as_mut().unwrap().step = Step::Evaluate;
                    self.command(p, Cmd::Step, in_ctx, ctl);
                } else {
                    self.get_device_desc(p);
                }
            }
            Step::DevDesc => {
                let Some(desc) = usb::parse_device(d.data_slice(18)) else {
                    self.fail(p, "bad device descriptor");
                    return;
                };
                d.desc = desc;
                if desc.class == usb::CLASS_HUB {
                    crate::println!(
                        "{}: port {}: hub {:04x}:{:04x} (hubs are not supported)",
                        self.name,
                        p + 1,
                        desc.vendor,
                        desc.product
                    );
                    d.step = Step::Unused;
                    return;
                }
                d.step = Step::ConfigHead;
                self.control(p, Setup::get_descriptor(usb::DESC_CONFIG, 0, 9));
            }
            Step::ConfigHead => {
                let h = d.data_slice(9);
                let total = u16::from_le_bytes([h[2], h[3]]).clamp(9, REPORT_BUF as u16);
                d.step = Step::Config;
                self.control(p, Setup::get_descriptor(usb::DESC_CONFIG, 0, total));
            }
            Step::Config => {
                let (value, ifaces) = usb::parse_config(d.data_slice(REPORT_BUF));
                let desc = d.desc;
                if ifaces.is_empty() {
                    crate::println!(
                        "{}: port {}: device {:04x}:{:04x} class {:#x} (no driver)",
                        self.name,
                        p + 1,
                        desc.vendor,
                        desc.product,
                        desc.class
                    );
                    d.step = Step::Unused;
                    return;
                }
                d.config = value;
                for (n, f) in ifaces.into_iter().take(MAX_HID).enumerate() {
                    let Some(kind) = hid::Kind::from_protocol(f.protocol) else { continue };
                    let ring = Ring::new(d.page(4 + n));
                    let dci = (f.endpoint & 0xf) * 2 + 1;
                    let len = f.max_packet.clamp(1, REPORT_MAX as u16);
                    d.hids.push(Hid { iface: f, kind, dci, ring, len, handle: 0, errors: 0 });
                }
                d.step = Step::SetConfig;
                self.control(p, Setup::set_configuration(value));
            }
            Step::SetConfig => {
                d.step = Step::SetProtocol(0);
                let iface = d.hids[0].iface.iface;
                self.control(p, Setup::set_protocol(iface, 0));
            }
            Step::SetProtocol(i) => {
                d.step = Step::SetIdle(i);
                let iface = d.hids[i as usize].iface.iface;
                self.control(p, Setup::set_idle(iface));
            }
            Step::SetIdle(i) => {
                if (i as usize) + 1 < d.hids.len() {
                    d.step = Step::SetProtocol(i + 1);
                    let iface = d.hids[i as usize + 1].iface.iface;
                    self.control(p, Setup::set_protocol(iface, 0));
                } else {
                    self.configure_endpoints(p);
                }
            }
            Step::Running => {
                d.ctl_busy = false;
                crate::trace!("usb: slot {} LEDs set to {:#x}", d.slot, d.leds);
            }
            _ => {}
        }
    }

    /// xHCI endpoint interval (2^n * 125 us) for a HID endpoint.
    fn interval(speed: u8, b_interval: u8) -> u32 {
        if speed == usb::speed::FULL || speed == usb::speed::LOW {
            // bInterval in frames (ms): log2(ms * 8).
            let us125 = (b_interval.max(1) as u32) * 8;
            (31 - us125.leading_zeros()).clamp(3, 10)
        } else {
            (b_interval.clamp(1, 16) - 1) as u32
        }
    }

    fn configure_endpoints(&mut self, p: usize) {
        let d = self.devs[p].as_ref().unwrap();
        self.clear_input(d);
        let mut add = 1u32;
        let mut max_dci = 1;
        for h in &d.hids {
            add |= 1 << h.dci;
            max_dci = max_dci.max(h.dci as u32);
            let ep = self.ctx(d.in_ctx(), 1 + h.dci as usize);
            let mps = h.iface.max_packet as u32;
            let ring = h.ring.base | 1;
            unsafe {
                write_volatile(ep, Self::interval(d.speed, h.iface.interval) << 16);
                write_volatile(ep.add(1), 3 << 1 | EP_INTERRUPT_IN << 3 | mps << 16);
                write_volatile(ep.add(2), ring as u32);
                write_volatile(ep.add(3), (ring >> 32) as u32);
                write_volatile(ep.add(4), mps << 16 | mps);
            }
        }
        unsafe { write_volatile(self.ctx(d.in_ctx(), 0).add(1), add) };
        self.slot_context(d, p, max_dci);
        let ctl = TRB_CONFIGURE << 10 | (d.slot as u32) << 24;
        let in_ctx = d.in_ctx();
        self.devs[p].as_mut().unwrap().step = Step::Configure;
        self.command(p, Cmd::Step, in_ctx, ctl);
    }

    fn start_running(&mut self, p: usize) {
        let name = self.name.clone();
        let d = self.devs[p].as_mut().unwrap();
        d.step = Step::Running;
        // Force the first LED update.
        d.leds = 0xff;
        for h in d.hids.iter_mut() {
            h.handle = hid::attach(h.kind);
            crate::println!(
                "{name}: port {}: USB {:?} {:04x}:{:04x} (slot {}, endpoint {:#x})",
                p + 1,
                h.kind,
                d.desc.vendor,
                d.desc.product,
                d.slot,
                h.iface.endpoint
            );
        }
        for i in 0..d.hids.len() {
            self.queue_report(p, i);
        }
    }

    /// Queue the next interrupt-IN transfer of HID interface `i`.
    fn queue_report(&mut self, p: usize, i: usize) {
        let Some(d) = self.devs[p].as_mut() else { return };
        let buf = d.data() + (REPORT_BUF + i * REPORT_MAX) as u64;
        let slot = d.slot;
        let Some(h) = d.hids.get_mut(i) else { return };
        h.ring.push(buf, h.len as u32, TRB_NORMAL << 10 | TRB_IOC | TRB_ISP);
        let dci = h.dci;
        self.ring_doorbell(slot, dci);
    }

    /// Keep keyboard LEDs in step with the console lock state.
    fn leds(&mut self) {
        let want = hid::leds();
        for p in 0..self.ports {
            let Some(d) = self.devs[p].as_mut() else { continue };
            if d.step != Step::Running || d.ctl_busy || d.leds == want {
                continue;
            }
            let Some(h) = d.hids.iter().find(|h| h.kind == hid::Kind::Keyboard) else { continue };
            let iface = h.iface.iface;
            d.leds = want;
            d.ctl_busy = true;
            unsafe { write_volatile(phys::phys_to_virt(d.data()), want) };
            self.control(p, Setup::set_leds(iface));
        }
    }
}

fn free_pages(mem: u64) {
    for i in 0..DEV_PAGES {
        phys::free(mem + (i * PAGE) as u64);
    }
}
