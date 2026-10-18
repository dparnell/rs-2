//! Character devices: the serial ports COM1-COM4 (16550 UARTs), the
//! printer ports LPT1-LPT3 and CLOCK$.
//!
//! COM ports receive on their interrupt (IRQ 4 for COM1/COM3, IRQ 3 for
//! COM2/COM4) into a buffer and transmit polled. COM1 is also the
//! kernel's debug log: programs may open it, but the log keeps writing to
//! it, so tests use COM2. The DCB read timeout modes (normal, wait for
//! something, no wait) are honoured for DosRead.
//!
//! The printer ports are sinks: nothing is attached to them in the
//! machines rs2 runs on. CLOCK$ reads and writes the DOS clock record:
//! days since 1980-01-01, minutes, hours, hundredths, seconds.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::api::err;
use crate::arch::idt::TrapFrame;
use crate::arch::{inb, outb};
use crate::sync::SpinLock;
use crate::task::WaitQueue;

const BASES: [u16; 4] = [0x3f8, 0x2f8, 0x3e8, 0x2e8];
const IRQS: [u8; 4] = [4, 3, 4, 3];
const RX_MAX: usize = 4096;

/// DCBINFO (11 bytes).
#[derive(Clone, Copy)]
pub struct Dcb {
    pub write_timeout: u16,
    pub read_timeout: u16,
    pub handshake: u8,
    pub flow: u8,
    pub timeout: u8,
    pub error_char: u8,
    pub break_char: u8,
    pub xon: u8,
    pub xoff: u8,
}

impl Dcb {
    pub fn bytes(&self) -> [u8; 11] {
        let w = self.write_timeout.to_le_bytes();
        let r = self.read_timeout.to_le_bytes();
        [w[0], w[1], r[0], r[1], self.handshake, self.flow, self.timeout, self.error_char, self.break_char, self.xon, self.xoff]
    }

    pub fn from_bytes(b: &[u8]) -> Dcb {
        Dcb {
            write_timeout: u16::from_le_bytes([b[0], b[1]]),
            read_timeout: u16::from_le_bytes([b[2], b[3]]),
            handshake: b[4],
            flow: b[5],
            timeout: b[6],
            error_char: b[7],
            break_char: b[8],
            xon: b[9],
            xoff: b[10],
        }
    }
}

struct Port {
    present: bool,
    baud: u32,
    data_bits: u8,
    /// 0 none, 1 odd, 2 even, 3 mark, 4 space.
    parity: u8,
    /// 0 = 1, 1 = 1.5, 2 = 2 stop bits.
    stop_bits: u8,
    brk: bool,
    mcr: u8,
    dcb: Dcb,
    rx: VecDeque<u8>,
    /// COM error word (RX_QUE_OVERRUN, RX_HARDWARE_OVERRUN, PARITY_ERROR,
    /// FRAMING_ERROR) and COM event word since last asked.
    errors: u16,
    events: u16,
    irq_on: bool,
}

const DEFAULT_DCB: Dcb = Dcb {
    write_timeout: 6000,
    read_timeout: 6000,
    handshake: 0x01, // DTR control
    flow: 0x40,      // RTS control
    timeout: 0x02,   // normal read timeout
    error_char: 0,
    break_char: 0,
    xon: 0x11,
    xoff: 0x13,
};

const fn port() -> Port {
    Port {
        present: false,
        baud: 9600,
        data_bits: 8,
        parity: 0,
        stop_bits: 0,
        brk: false,
        mcr: 0x0b,
        dcb: DEFAULT_DCB,
        rx: VecDeque::new(),
        errors: 0,
        events: 0,
        irq_on: false,
    }
}

static PORTS: [SpinLock<Port>; 4] = [SpinLock::new(port()), SpinLock::new(port()), SpinLock::new(port()), SpinLock::new(port())];
static RX_WAIT: WaitQueue = WaitQueue::new();

/// Find the UARTs (scratch register test). COM1 keeps the debug log's
/// 115200 8N1 setup.
pub fn init() {
    for i in 0..4 {
        let b = BASES[i];
        let present = unsafe {
            outb(b + 7, 0x5a);
            let a = inb(b + 7) == 0x5a;
            outb(b + 7, 0xa5);
            a && inb(b + 7) == 0xa5
        };
        let mut p = PORTS[i].lock();
        p.present = present;
        if !present {
            continue;
        }
        if i == 0 {
            p.baud = 115200;
        } else {
            program(b, &p);
        }
    }
    let found: Vec<usize> = (0..4).filter(|&i| PORTS[i].lock().present).map(|i| i + 1).collect();
    crate::println!("serial ports: {:?}", found);
}

/// Write the line settings to the UART.
fn program(base: u16, p: &Port) {
    let div = (115200 / p.baud.max(1)).clamp(1, 0xffff) as u16;
    let parity = match p.parity {
        1 => 0x08,
        2 => 0x18,
        3 => 0x28,
        4 => 0x38,
        _ => 0,
    };
    let lcr = (p.data_bits.clamp(5, 8) - 5) | if p.stop_bits > 0 { 4 } else { 0 } | parity | if p.brk { 0x40 } else { 0 };
    unsafe {
        outb(base + 3, 0x80);
        outb(base, div as u8);
        outb(base + 1, (div >> 8) as u8);
        outb(base + 3, lcr);
        outb(base + 2, 0xc1); // FIFO on, 14-byte trigger (pending input kept)
        outb(base + 4, p.mcr | 0x08);
        outb(base + 1, if p.irq_on { 0x01 } else { 0x00 });
    }
}

fn irq3(_f: &mut TrapFrame) {
    service(1);
    service(3);
}

fn irq4(_f: &mut TrapFrame) {
    service(0);
    service(2);
}

/// Move received bytes into the buffer.
fn service(i: usize) {
    let mut p = PORTS[i].lock();
    if !p.present || !p.irq_on {
        return;
    }
    let b = BASES[i];
    let mut got = false;
    unsafe {
        let _ = inb(b + 2);
        for _ in 0..64 {
            let lsr = inb(b + 5);
            if lsr & 0x1e != 0 {
                p.errors |= ((lsr & 0x02 != 0) as u16) << 1 | ((lsr & 0x04 != 0) as u16) << 2 | ((lsr & 0x08 != 0) as u16) << 3;
                if lsr & 0x10 != 0 {
                    p.events |= 0x0040; // BREAK_DETECTED
                }
                if lsr & 0x0e != 0 {
                    p.events |= 0x0080; // ERROR_OCCURRED
                }
            }
            if lsr & 1 == 0 {
                break;
            }
            let c = inb(b);
            if p.rx.len() >= RX_MAX {
                p.errors |= 0x0001; // RX_QUE_OVERRUN
                continue;
            }
            p.rx.push_back(c);
            p.events |= 0x0001; // CHAR_RECEIVED
            got = true;
        }
        let _ = inb(b + 6);
    }
    drop(p);
    if got {
        RX_WAIT.wake_all();
    }
}

/// Port number (0-based) for a COMn device name.
pub fn com_index(name: &str) -> Option<usize> {
    let n = name.strip_prefix("COM")?.parse::<usize>().ok()?;
    (1..=4).contains(&n).then_some(n - 1)
}

pub fn com_present(i: usize) -> bool {
    i < 4 && PORTS[i].lock().present
}

/// A program opened COMn: start receiving.
pub fn com_open(i: usize) -> Result<(), u32> {
    if !com_present(i) {
        return Err(err::FILE_NOT_FOUND);
    }
    let first = {
        let mut p = PORTS[i].lock();
        let first = !p.irq_on;
        p.irq_on = true;
        if i == 0 {
            unsafe { outb(BASES[0] + 1, 0x01) };
        } else {
            program(BASES[i], &p);
        }
        first
    };
    if first {
        crate::irq::register(IRQS[i], if IRQS[i] == 3 { irq3 } else { irq4 });
    }
    Ok(())
}

/// DosRead from COMn.
pub fn com_read(i: usize, len: usize) -> Vec<u8> {
    let (mode, timeout) = {
        let p = PORTS[i].lock();
        (p.dcb.timeout & 0x06, (p.dcb.read_timeout as u64 + 1) * 10)
    };
    let mut out = Vec::new();
    let take = |out: &mut Vec<u8>| {
        let mut p = PORTS[i].lock();
        while out.len() < len {
            match p.rx.pop_front() {
                Some(c) => out.push(c),
                None => break,
            }
        }
    };
    take(&mut out);
    if mode == 0x06 {
        return out;
    }
    while out.len() < len {
        // Normal: the timeout runs from the last character received.
        let deadline = crate::irq::ticks() + timeout * crate::irq::HZ / 1000;
        let arrived = RX_WAIT.wait_until(Some(deadline), || !PORTS[i].lock().rx.is_empty());
        if !arrived {
            break;
        }
        take(&mut out);
        if mode == 0x04 {
            break;
        }
    }
    out
}

/// DosWrite to COMn: transmit polled.
pub fn com_write(i: usize, data: &[u8]) -> usize {
    let b = BASES[i];
    let mut n = 0;
    for &c in data {
        let mut spins = 0u32;
        while unsafe { inb(b + 5) } & 0x20 == 0 {
            spins += 1;
            if spins > 2_000_000 {
                return n;
            }
            core::hint::spin_loop();
        }
        unsafe { outb(b, c) };
        n += 1;
    }
    n
}

/// Category 1 (ASYNC) IOCtls on COMn; returns the data to give back.
pub fn com_ioctl(i: usize, f: u32, params: &[u8]) -> Result<Vec<u8>, u32> {
    let b = BASES[i];
    let mut p = PORTS[i].lock();
    let word = |v: u16| v.to_le_bytes().to_vec();
    let need = |n: usize| if params.len() < n { Err(err::INVALID_PARAMETER) } else { Ok(()) };
    let set_baud = |p: &mut Port, baud: u32| -> Result<(), u32> {
        if !(10..=115200).contains(&baud) || 115200 % baud > baud / 50 {
            return Err(err::INVALID_PARAMETER);
        }
        p.baud = baud;
        Ok(())
    };
    let r = match f {
        0x41 => {
            need(2)?;
            set_baud(&mut p, u16::from_le_bytes([params[0], params[1]]) as u32)?;
            Vec::new()
        }
        0x43 => {
            need(4)?;
            set_baud(&mut p, u32::from_le_bytes([params[0], params[1], params[2], params[3]]))?;
            Vec::new()
        }
        0x61 => word(p.baud.min(0xffff) as u16),
        0x63 => {
            let mut v = Vec::new();
            for x in [p.baud, 10, 115200] {
                v.extend_from_slice(&x.to_le_bytes());
                v.push(0);
            }
            v
        }
        0x42 => {
            need(3)?;
            let (d, par, st) = (params[0], params[1], params[2]);
            if !(5..=8).contains(&d) || par > 4 || st > 2 || (st == 1 && d != 5) || (st == 2 && d == 5) {
                return Err(err::INVALID_PARAMETER);
            }
            p.data_bits = d;
            p.parity = par;
            p.stop_bits = st;
            Vec::new()
        }
        0x62 => alloc::vec![p.data_bits, p.parity, p.stop_bits, p.brk as u8],
        0x44 => {
            need(1)?;
            drop(p);
            com_write(i, &params[..1]);
            return Ok(Vec::new());
        }
        0x45 | 0x4b => {
            p.brk = f == 0x4b;
            word(core::mem::take(&mut p.errors))
        }
        0x46 => {
            need(2)?;
            let (on, off) = (params[0] & 3, params[1] | !3);
            p.mcr = (p.mcr | on) & off;
            word(0)
        }
        0x47 | 0x48 => Vec::new(),
        0x53 => {
            need(11)?;
            p.dcb = Dcb::from_bytes(params);
            Vec::new()
        }
        0x73 => p.dcb.bytes().to_vec(),
        0x54 => Vec::new(),
        0x74 => alloc::vec![0],
        0x64 => alloc::vec![0],
        0x65 => alloc::vec![if unsafe { inb(b + 5) } & 0x40 == 0 { 0x04 } else { 0 }],
        0x66 => alloc::vec![p.mcr & 3],
        0x67 => alloc::vec![unsafe { inb(b + 6) } & 0xf0],
        0x68 => {
            let mut v = word(p.rx.len() as u16);
            v.extend_from_slice(&(RX_MAX as u16).to_le_bytes());
            v
        }
        0x69 => {
            let mut v = word(0);
            v.extend_from_slice(&(RX_MAX as u16).to_le_bytes());
            v
        }
        0x6d => word(core::mem::take(&mut p.errors)),
        0x72 => word(core::mem::take(&mut p.events)),
        _ => return Err(err::INVALID_FUNCTION),
    };
    // COM1 carries the kernel log too; it changes all the same.
    if matches!(f, 0x41 | 0x43 | 0x42 | 0x45 | 0x4b | 0x46) {
        program(b, &p);
    }
    Ok(r)
}

/// Category 0xB (general) IOCtls: flush the input (1) or output (2) buffer.
pub fn com_general(i: usize, f: u32) -> Result<(), u32> {
    match f {
        1 => {
            PORTS[i].lock().rx.clear();
            Ok(())
        }
        2 => Ok(()),
        _ => Err(err::INVALID_FUNCTION),
    }
}

// ---- Printer ports ----

static LPT_BYTES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Output to LPTn (0-based): no printer is attached, so it goes nowhere
/// (it is counted, and traced with `apitrace`).
pub fn lpt_write(n: usize, data: &[u8]) -> usize {
    LPT_BYTES.fetch_add(data.len() as u64, core::sync::atomic::Ordering::Relaxed);
    if crate::api::tracing() {
        crate::trace!("LPT{}: {} bytes", n + 1, data.len());
    }
    data.len()
}

/// Category 5 (printer) IOCtls.
pub fn lpt_ioctl(f: u32) -> Result<Vec<u8>, u32> {
    match f {
        // PRT_GETPRINTERSTATUS: selected, not busy.
        0x66 => Ok(alloc::vec![0x90]),
        // PRT_GETFRAMECTL: 80 characters per line, 6 lines per inch.
        0x62 => Ok(alloc::vec![80, 6]),
        // PRT_GETINFINITERETRY
        0x64 => Ok(alloc::vec![0]),
        0x42 | 0x44 | 0x46 | 0x48 | 0x4d | 0x4e | 0x52 | 0x53 => Ok(Vec::new()),
        _ => Err(err::INVALID_FUNCTION),
    }
}

// ---- CLOCK$ ----

fn days_from_1980(y: u16, m: u8, d: u8) -> u32 {
    let mut days = 0;
    for yy in 1980..y {
        days += if yy % 4 == 0 && (yy % 100 != 0 || yy % 400 == 0) { 366 } else { 365 };
    }
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    days += CUM[(m.clamp(1, 12) - 1) as usize] + d as u32 - 1;
    if m > 2 && y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
        days += 1;
    }
    days
}

/// The clock record.
pub fn clock_read() -> Vec<u8> {
    let t = crate::rtc::now();
    let days = days_from_1980(t.year, t.month, t.day) as u16;
    let mut v = days.to_le_bytes().to_vec();
    v.extend_from_slice(&[t.minute, t.hour, 0, t.second]);
    v
}

/// Set the clock from a clock record.
pub fn clock_write(b: &[u8]) -> Result<(), u32> {
    if b.len() < 6 || b[2] > 59 || b[3] > 23 || b[5] > 59 {
        return Err(err::INVALID_DATA);
    }
    let mut days = u16::from_le_bytes([b[0], b[1]]) as u32;
    let mut y = 1980u16;
    loop {
        let n = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if days < n {
            break;
        }
        days -= n;
        y += 1;
    }
    let mut m = 1u8;
    while m < 12 && days_from_1980(y, m + 1, 1) - days_from_1980(y, 1, 1) <= days {
        m += 1;
    }
    let d = (days - (days_from_1980(y, m, 1) - days_from_1980(y, 1, 1)) + 1) as u8;
    let t = crate::rtc::DateTime { year: y, month: m, day: d, hour: b[3], minute: b[2], second: b[5], weekday: crate::rtc::weekday(y, m, d) };
    crate::rtc::set(&t);
    Ok(())
}

// ---- Dispatch for file handles ----

use crate::fs::CharDev;

/// DosRead from a character device.
pub fn read(d: CharDev, len: usize) -> Vec<u8> {
    match d {
        CharDev::Com(n) => com_read(n as usize, len),
        _ => Vec::new(),
    }
}

/// DosWrite to a character device; returns the bytes written.
pub fn write(d: CharDev, data: &[u8]) -> usize {
    match d {
        CharDev::Com(n) => com_write(n as usize, data),
        CharDev::Lpt(n) => lpt_write(n as usize, data),
        _ => data.len(),
    }
}
