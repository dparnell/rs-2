//! The hardware side of rs2's parallel port driver (kernel `parport.rs`,
//! docs/PRINTING.md "The parallel port driver"), written over a port I/O
//! trait ([`Io`]) so that it runs on the host against a simulated port
//! (`cargo test -p parport`; QEMU's parallel port is a plain SPP port
//! with no IEEE 1284 peripheral behind it).
//!
//! - detection: the data latch, an ECP chip's extended control register
//!   (ECR, base+402h or a PCI card's second BAR + 2), EPP, PS/2
//!   bidirectional mode; probing only reads and writes registers, it
//!   never waits on the peripheral;
//! - output: compatibility mode a byte at a time (the printer's status
//!   lines, strobe), the ECP chip's compatibility FIFO (ECR mode 010:
//!   the chip strobes each byte), ECP mode (IEEE 1284 negotiation,
//!   setup, the ECP data FIFO, termination) and EPP data cycles;
//! - IEEE 1284 negotiation and termination, the reverse channel in
//!   nibble and byte mode, and the device ID read through it.
//!
//! The steps follow IEEE 1284 (event numbers in the comments) as IBM's
//! PAR1284.SYS names them (`_do_negotiate`, `_StatusToNibble`,
//! `_do_read_byte_mode`, `_do_write_fpp_mode`, `_do_write_ecp_mode`,
//! `_do_write_epp_mode`, `_DetectECP`, `_parDeviceIDQuery`; its symbol
//! file is on the Warp 4.52 CD, the driver itself is not). The code is
//! rs2's own.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;

// Control register (base+2). STROBE, AUTOFD and SELECTIN are inverted
// on the cable: a 1 drives the line low.
pub const CTR_STROBE: u8 = 0x01;
pub const CTR_AUTOFD: u8 = 0x02;
pub const CTR_INIT: u8 = 0x04;
pub const CTR_SELECT: u8 = 0x08;
pub const CTR_INTEN: u8 = 0x10;
pub const CTR_DIR: u8 = 0x20;
/// Idle: not resetting, printer selected.
pub const CTR_IDLE: u8 = CTR_INIT | CTR_SELECT;

// Status register (base+1), as read: BUSY reads 0 while busy, ACK and
// ERROR are active low.
pub const STS_EPP_TIMEOUT: u8 = 0x01;
pub const STS_ERROR: u8 = 0x08;
pub const STS_SELECT: u8 = 0x10;
pub const STS_PAPER: u8 = 0x20;
pub const STS_ACK: u8 = 0x40;
pub const STS_BUSY: u8 = 0x80;

// Extended control register modes (bits 7-5) and bits.
pub const ECR_SPP: u8 = 0x00;
pub const ECR_PS2: u8 = 0x20;
pub const ECR_FIFO: u8 = 0x40;
pub const ECR_ECP: u8 = 0x60;
pub const ECR_EPP: u8 = 0x80;
/// nErrIntrEn and serviceIntr set: no ECP interrupts.
pub const ECR_NOINTR: u8 = 0x14;
pub const ECR_EMPTY: u8 = 0x01;
pub const ECR_FULL: u8 = 0x02;

/// Capabilities beyond compatibility and nibble mode (every port).
pub const CAP_BYTE: u8 = 0x01;
pub const CAP_EPP: u8 = 0x02;
pub const CAP_ECP: u8 = 0x04;

/// IEEE 1284 extensibility request values.
pub const XFLAG_NIBBLE: u8 = 0x00;
pub const XFLAG_BYTE: u8 = 0x01;
pub const XFLAG_DEVICE_ID: u8 = 0x04;
pub const XFLAG_ECP: u8 = 0x10;

// Errors (OS/2 error codes; device errors are 19 + the driver's code).
/// Device error 1 (unknown unit): PRINT01's for a printer that answers
/// with "not busy" but is not ready.
pub const BAD_UNIT: u32 = 20;
/// The peripheral does not negotiate (or refuses) the IEEE 1284 mode.
pub const BAD_COMMAND: u32 = 22;
pub const OUT_OF_PAPER: u32 = 28;
pub const WRITE_FAULT: u32 = 29;
/// The thread is to end (ERROR_INTERRUPT).
pub const INTERRUPT: u32 = 95;

/// IEEE 1284's response time for each handshake step, ms.
pub const T_RESPONSE: u64 = 35;
/// The longest device ID read.
pub const MAX_ID: usize = 4096;

/// A port's registers: the base (data, status, control; EPP address and
/// data at +3/+4) and the ECP chip's extended control register (0:
/// none). The ECP data FIFO is at ECR - 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    pub base: u16,
    pub ecr: u16,
}

impl Port {
    pub const fn data(&self) -> u16 {
        self.base
    }
    pub const fn status(&self) -> u16 {
        self.base + 1
    }
    pub const fn control(&self) -> u16 {
        self.base + 2
    }
    pub const fn epp_addr(&self) -> u16 {
        self.base + 3
    }
    pub const fn epp_data(&self) -> u16 {
        self.base + 4
    }
    pub const fn fifo(&self) -> u16 {
        self.ecr - 2
    }
}

/// Port I/O and waiting, as the kernel (or a host test's simulated port)
/// provides them.
pub trait Io {
    fn inb(&mut self, port: u16) -> u8;
    fn outb(&mut self, port: u16, v: u8);
    /// A deadline `ms` milliseconds from now.
    fn deadline(&mut self, ms: u64) -> u64;
    fn expired(&mut self, deadline: u64) -> bool;
    /// What tells a waiter that the port interrupted since (an interrupt
    /// count); read before a condition is checked, so that an
    /// interrupt after the check ends the pause that follows.
    fn mark(&mut self) -> u64 {
        0
    }
    /// Wait a little: until `deadline` at most, an interrupt after
    /// `mark`, or a timer tick when the port is polled. False when the
    /// thread is to end.
    fn pause(&mut self, deadline: u64, mark: u64) -> bool;
    /// Before byte `i` of a compatibility-mode write (the kernel's
    /// `lptstall=` test stalls).
    fn before_byte(&mut self, _i: usize) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waited {
    Ready,
    Timeout,
    Killed,
}

/// Wait up to `ms` for `cond`: `spins` quick polls first (a port read
/// takes about a microsecond), then pausing between checks.
pub fn wait_for<I: Io + ?Sized>(io: &mut I, ms: u64, spins: u32, mut cond: impl FnMut(&mut I) -> bool) -> Waited {
    for _ in 0..spins {
        if cond(io) {
            return Waited::Ready;
        }
    }
    let d = io.deadline(ms);
    loop {
        let m = io.mark();
        if cond(io) {
            return Waited::Ready;
        }
        if io.expired(d) {
            return if cond(io) { Waited::Ready } else { Waited::Timeout };
        }
        if !io.pause(d, m) {
            return Waited::Killed;
        }
    }
}

/// A short wait on a handshake line (32 quick polls).
fn wait_line<I: Io + ?Sized>(io: &mut I, p: &Port, ms: u64, mask: u8, val: u8) -> bool {
    let s = p.status();
    wait_for(io, ms, 32, |io| io.inb(s) & mask == val) == Waited::Ready
}

/// About `n` microseconds (status reads).
fn settle<I: Io + ?Sized>(io: &mut I, p: &Port, n: u32) {
    for _ in 0..n {
        let _ = io.inb(p.status());
    }
}

// ---- Detection ----

/// The data latch reads back what was written (a port is there).
pub fn latch_ok<I: Io + ?Sized>(io: &mut I, p: &Port) -> bool {
    let ctr = io.inb(p.control());
    io.outb(p.control(), ctr & !CTR_DIR);
    io.outb(p.data(), 0x55);
    let a = io.inb(p.data());
    io.outb(p.data(), 0xaa);
    let b = io.inb(p.data());
    a == 0x55 && b == 0xaa
}

/// An ECP chip's extended control register at `ecr` (the test Linux's
/// parport_pc uses: a register that only mirrors the control register is
/// not one; after a reset the FIFO is empty and mode writes read back).
/// Leaves the chip in PS/2 mode.
pub fn ecr_ok<I: Io + ?Sized>(io: &mut I, base: u16, ecr: u16) -> bool {
    let ctl = base + 2;
    let ctr = io.inb(ctl);
    let e = io.inb(ecr);
    if e & 3 == ctr & 3 {
        io.outb(ctl, ctr ^ CTR_AUTOFD);
        let c2 = io.inb(ctl);
        let mirrors = io.inb(ecr) & 2 == c2 & 2;
        io.outb(ctl, ctr);
        if mirrors {
            return false;
        }
    }
    if io.inb(ecr) & 3 != ECR_EMPTY {
        return false;
    }
    io.outb(ecr, 0x34);
    let ok = io.inb(ecr) == 0x35;
    io.outb(ecr, ECR_PS2 | ECR_NOINTR);
    ok
}

/// Clear the EPP timeout bit (chips clear it on a read, by writing 1, or
/// by writing 0); whether it is clear now.
pub fn clear_epp_timeout<I: Io + ?Sized>(io: &mut I, p: &Port) -> bool {
    let s = p.status();
    if io.inb(s) & STS_EPP_TIMEOUT == 0 {
        return true;
    }
    let _ = io.inb(s);
    let r = io.inb(s);
    io.outb(s, r | STS_EPP_TIMEOUT);
    io.outb(s, r & !STS_EPP_TIMEOUT);
    io.inb(s) & STS_EPP_TIMEOUT == 0
}

/// EPP: an address cycle with nobody answering times out (the status
/// timeout bit sets), which an SPP port never reports.
pub fn epp_ok<I: Io + ?Sized>(io: &mut I, p: &Port) -> bool {
    if !clear_epp_timeout(io, p) {
        return false;
    }
    io.outb(p.control(), CTR_INIT);
    let _ = io.inb(p.epp_addr());
    let t = io.inb(p.status()) & STS_EPP_TIMEOUT != 0;
    clear_epp_timeout(io, p);
    io.outb(p.control(), CTR_IDLE);
    t
}

/// Bidirectional (PS/2 byte mode): with the direction bit set the data
/// lines float and no longer read back the latch.
pub fn byte_ok<I: Io + ?Sized>(io: &mut I, p: &Port) -> bool {
    let ctr = io.inb(p.control());
    io.outb(p.control(), ctr | CTR_DIR);
    io.outb(p.data(), 0x55);
    let a = io.inb(p.data());
    io.outb(p.data(), 0xaa);
    let b = io.inb(p.data());
    io.outb(p.control(), ctr & !CTR_DIR);
    !(a == 0x55 && b == 0xaa)
}

/// What the port can do (the ECP chip's modes switched for the EPP and
/// byte tests, PS/2 mode after). Leaves the port idle.
pub fn capabilities<I: Io + ?Sized>(io: &mut I, p: &Port) -> u8 {
    let mut caps = 0;
    if p.ecr != 0 {
        caps |= CAP_ECP;
        io.outb(p.ecr, ECR_EPP | ECR_NOINTR);
        if epp_ok(io, p) {
            caps |= CAP_EPP;
        }
        io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    } else if epp_ok(io, p) {
        caps |= CAP_EPP;
    }
    if byte_ok(io, p) {
        caps |= CAP_BYTE;
    }
    io.outb(p.control(), CTR_IDLE);
    caps
}

// ---- Status ----

/// PRINT01's view of the status lines: ACK and ERROR made active high.
pub fn ibm_status(raw: u8) -> u8 {
    (raw & 0xf8) ^ (STS_ACK | STS_ERROR)
}

/// PRINT01's error for a printer that stopped (`prterp`): its status
/// lines say out of paper (28), a fault or nothing answering (29 write
/// fault), or a printer that is not busy but not ready either (20).
pub fn stop_error(raw: u8) -> u32 {
    let ah = ibm_status(raw);
    let (nb, pe, sel, fault) = (ah & 0x80 != 0, ah & 0x20 != 0, ah & 0x10 != 0, ah & 0x08 != 0);
    if nb && pe && sel {
        BAD_UNIT
    } else if !nb && pe && ((sel && !fault) || (!sel && fault)) {
        OUT_OF_PAPER
    } else if !nb && (sel || fault) || ah == 0 {
        WRITE_FAULT
    } else {
        BAD_UNIT
    }
}

/// The printer takes a byte: not busy, selected, paper, no fault (or
/// not selected while sending DC1, which selects it).
pub fn ready(raw: u8, byte: u8) -> bool {
    let ah = ibm_status(raw);
    ah == 0x90 || (ah == 0x80 && byte == 0x11)
}

// ---- Output ----

/// Compatibility mode, a byte at a time: wait until the printer is
/// ready, data, strobe. `ctr` is the idle control value (with the
/// interrupt enable when the acknowledge interrupts). With `infinite`
/// a printer that stops is waited for as long as it takes.
pub fn spp_write<I: Io + ?Sized>(io: &mut I, p: &Port, ctr: u8, data: &[u8], timeout_ms: u64, infinite: bool) -> (usize, Result<(), u32>) {
    if p.ecr != 0 {
        io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    }
    io.outb(p.control(), ctr);
    let s = p.status();
    for (i, &b) in data.iter().enumerate() {
        io.before_byte(i);
        loop {
            match wait_for(io, timeout_ms, 32, |io| ready(io.inb(s), b)) {
                Waited::Ready => break,
                Waited::Killed => return (i, Err(INTERRUPT)),
                // Infinite retry: PRINT01 keeps waiting and says nothing.
                Waited::Timeout if infinite => continue,
                Waited::Timeout => return (i, Err(stop_error(io.inb(s)))),
            }
        }
        io.outb(p.data(), b);
        // The data settles (at least 0.5 us), a strobe of at least
        // 0.5 us: port reads take about a microsecond.
        settle(io, p, 1);
        io.outb(p.control(), ctr | CTR_STROBE);
        settle(io, p, 2);
        io.outb(p.control(), ctr);
    }
    // The last byte taken (or the interrupt), briefly.
    let _ = wait_for(io, 10, 32, |io| io.inb(s) & STS_BUSY != 0);
    (data.len(), Ok(()))
}

/// Quick polls of the FIFO before pausing: about 2 ms of a printer
/// taking bytes (a polled port pauses a timer tick at a time).
const FIFO_SPINS: u32 = 2000;

/// Bytes into the chip's FIFO (the FIFO register at ECR - 2) as it has
/// room, then until it has drained.
fn fifo_out<I: Io + ?Sized>(io: &mut I, p: &Port, data: &[u8], timeout_ms: u64) -> (usize, Result<(), u32>) {
    let (ecr, fifo, s) = (p.ecr, p.fifo(), p.status());
    for (i, &b) in data.iter().enumerate() {
        match wait_for(io, timeout_ms, FIFO_SPINS, |io| io.inb(ecr) & ECR_FULL == 0) {
            Waited::Ready => io.outb(fifo, b),
            Waited::Killed => return (i, Err(INTERRUPT)),
            Waited::Timeout => return (i, Err(stop_error(io.inb(s)))),
        }
    }
    // Drained, and the peripheral took the last byte.
    match wait_for(io, timeout_ms, FIFO_SPINS, |io| io.inb(ecr) & ECR_EMPTY != 0) {
        Waited::Ready => {}
        Waited::Killed => return (data.len(), Err(INTERRUPT)),
        // Bytes left in the FIFO did not reach the printer: the count
        // is only an upper bound, as for the chips' own FIFO counts.
        Waited::Timeout => return (data.len(), Err(stop_error(io.inb(s)))),
    }
    let _ = wait_for(io, 10, 32, |io| io.inb(s) & STS_BUSY != 0);
    (data.len(), Ok(()))
}

/// Compatibility mode through the ECP chip's FIFO (ECR mode 010, IBM's
/// "fast parallel port"): the chip waits for the printer and strobes
/// each byte itself.
pub fn fifo_write<I: Io + ?Sized>(io: &mut I, p: &Port, data: &[u8], timeout_ms: u64) -> (usize, Result<(), u32>) {
    io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    io.outb(p.control(), CTR_IDLE);
    io.outb(p.ecr, ECR_FIFO | ECR_NOINTR);
    let r = fifo_out(io, p, data, timeout_ms);
    io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    io.outb(p.control(), CTR_IDLE);
    r
}

/// ECP mode: negotiate it (extensibility byte 10h), the setup phase
/// (event 30: HostAck low; event 31: the peripheral raises
/// nAckReverse), then the chip's ECP data FIFO; terminated after.
/// A peripheral that does not negotiate ECP gives BAD_COMMAND with
/// nothing sent (the caller falls back to the compatibility FIFO).
pub fn ecp_write<I: Io + ?Sized>(io: &mut I, p: &Port, data: &[u8], timeout_ms: u64) -> (usize, Result<(), u32>) {
    io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    if let Err(e) = negotiate(io, p, XFLAG_ECP) {
        return (0, Err(e));
    }
    // Event 30: HostAck (nAutoFd) low; event 31: PError high.
    io.outb(p.control(), CTR_INIT | CTR_AUTOFD);
    if !wait_line(io, p, T_RESPONSE, STS_PAPER, STS_PAPER) {
        terminate(io, p);
        return (0, Err(BAD_COMMAND));
    }
    // Forward idle: the chip drives HostClk and HostAck from here.
    io.outb(p.ecr, ECR_ECP | ECR_NOINTR);
    let r = fifo_out(io, p, data, timeout_ms);
    io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    terminate(io, p);
    r
}

/// EPP: each byte through the EPP data register; the chip handshakes
/// and reports a peripheral that does not answer as a timeout
/// (WRITE_FAULT, with the bytes taken before it).
pub fn epp_write<I: Io + ?Sized>(io: &mut I, p: &Port, data: &[u8]) -> (usize, Result<(), u32>) {
    if p.ecr != 0 {
        io.outb(p.ecr, ECR_EPP | ECR_NOINTR);
    }
    io.outb(p.control(), CTR_INIT);
    clear_epp_timeout(io, p);
    let mut r = (data.len(), Ok(()));
    for (i, &b) in data.iter().enumerate() {
        io.outb(p.epp_data(), b);
        if io.inb(p.status()) & STS_EPP_TIMEOUT != 0 {
            clear_epp_timeout(io, p);
            r = (i, Err(WRITE_FAULT));
            break;
        }
    }
    if p.ecr != 0 {
        io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    }
    io.outb(p.control(), CTR_IDLE);
    r
}

// ---- IEEE 1284 ----

/// Negotiate extensibility mode `xflag` (events 0-6): the peripheral
/// answers within 35 ms (nAck low, PError, Select and nFault high) and
/// says with Select whether it supports the mode (nibble mode, 00h, is
/// accepted with Select low, the others high). A peripheral that does
/// not answer is left in compatibility mode; one that refuses is
/// terminated. BAD_COMMAND either way.
pub fn negotiate<I: Io + ?Sized>(io: &mut I, p: &Port, xflag: u8) -> Result<(), u32> {
    io.outb(p.control(), CTR_IDLE);
    // Event 0: the request on the data lines; event 1: nSelectIn high,
    // nAutoFd low.
    io.outb(p.data(), xflag);
    settle(io, p, 1);
    io.outb(p.control(), CTR_INIT | CTR_AUTOFD);
    // Event 2: nAck low, PError, Select and nFault high.
    let m = STS_ACK | STS_PAPER | STS_SELECT | STS_ERROR;
    if !wait_line(io, p, T_RESPONSE, m, STS_PAPER | STS_SELECT | STS_ERROR) {
        io.outb(p.control(), CTR_IDLE);
        return Err(BAD_COMMAND);
    }
    // Event 3: strobe the request in; event 4: nStrobe and nAutoFd high.
    io.outb(p.control(), CTR_INIT | CTR_AUTOFD | CTR_STROBE);
    settle(io, p, 1);
    io.outb(p.control(), CTR_INIT);
    // Events 5-6: PError low, Select the answer, nAck high.
    if !wait_line(io, p, T_RESPONSE, STS_ACK, STS_ACK) {
        terminate(io, p);
        return Err(BAD_COMMAND);
    }
    let sel = io.inb(p.status()) & STS_SELECT != 0;
    if (xflag == XFLAG_NIBBLE) == sel {
        terminate(io, p);
        return Err(BAD_COMMAND);
    }
    Ok(())
}

/// Back to compatibility mode (events 22-29): nSelectIn low and nAutoFd
/// high, the peripheral pulls nAck low; nAutoFd low, nAck high; nAutoFd
/// high. The data lines driven again.
pub fn terminate<I: Io + ?Sized>(io: &mut I, p: &Port) {
    io.outb(p.control(), CTR_INIT | CTR_SELECT);
    let _ = wait_line(io, p, T_RESPONSE, STS_ACK, 0);
    io.outb(p.control(), CTR_INIT | CTR_SELECT | CTR_AUTOFD);
    let _ = wait_line(io, p, T_RESPONSE, STS_ACK, STS_ACK);
    io.outb(p.control(), CTR_IDLE);
}

/// The four data bits of a nibble-mode status read (PAR1284's
/// `_StatusToNibble`): nFault, Select, PError and Busy (the register's
/// bit 7 is the inverted Busy line).
pub fn status_to_nibble(s: u8) -> u8 {
    (s >> 3 & 1) | (s >> 4 & 1) << 1 | (s >> 5 & 1) << 2 | ((s & STS_BUSY == 0) as u8) << 3
}

/// One byte in nibble mode (low nibble first): HostBusy (nAutoFd) low
/// (event 7), PtrClk (nAck) low (event 9) with the nibble on the
/// status lines, HostBusy high (event 10), PtrClk high (event 11).
pub fn nibble_byte<I: Io + ?Sized>(io: &mut I, p: &Port) -> Option<u8> {
    let mut v = 0u8;
    for half in 0..2 {
        io.outb(p.control(), CTR_INIT | CTR_AUTOFD);
        if !wait_line(io, p, T_RESPONSE, STS_ACK, 0) {
            return None;
        }
        v |= status_to_nibble(io.inb(p.status())) << (4 * half);
        io.outb(p.control(), CTR_INIT);
        if !wait_line(io, p, T_RESPONSE, STS_ACK, STS_ACK) {
            return None;
        }
    }
    Some(v)
}

/// One byte in byte mode: the data lines turned around (event 14),
/// HostBusy low (event 7), PtrClk low (event 9) with the byte on the
/// data lines, HostBusy high (event 10), PtrClk high (event 11), then
/// HostClk (nStrobe) pulsed to acknowledge it (events 16-17).
pub fn byte_mode_byte<I: Io + ?Sized>(io: &mut I, p: &Port) -> Option<u8> {
    io.outb(p.control(), CTR_INIT | CTR_DIR | CTR_AUTOFD);
    if !wait_line(io, p, T_RESPONSE, STS_ACK, 0) {
        return None;
    }
    let v = io.inb(p.data());
    io.outb(p.control(), CTR_INIT | CTR_DIR);
    if !wait_line(io, p, T_RESPONSE, STS_ACK, STS_ACK) {
        return None;
    }
    io.outb(p.control(), CTR_INIT | CTR_DIR | CTR_STROBE);
    settle(io, p, 5);
    io.outb(p.control(), CTR_INIT | CTR_DIR);
    Some(v)
}

/// The reverse channels the device ID is read through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reverse {
    Nibble,
    Byte,
}

/// Read the IEEE 1284 device ID through `mode` (extensibility request
/// 04h, 05h for byte mode): two bytes of length (big-endian, counting
/// themselves), then `MFG:...;MDL:...;`. Reading stops at the length or
/// when the peripheral says no more data (nDataAvail, nFault, high).
pub fn read_device_id<I: Io + ?Sized>(io: &mut I, p: &Port, mode: Reverse) -> Result<Vec<u8>, u32> {
    if p.ecr != 0 {
        io.outb(p.ecr, ECR_PS2 | ECR_NOINTR);
    }
    let xflag = XFLAG_DEVICE_ID | if mode == Reverse::Byte { XFLAG_BYTE } else { XFLAG_NIBBLE };
    negotiate(io, p, xflag)?;
    let mut out = Vec::new();
    let get = |io: &mut I| match mode {
        Reverse::Nibble => nibble_byte(io, p),
        Reverse::Byte => byte_mode_byte(io, p),
    };
    let r = (|| {
        let hi = get(io).ok_or(BAD_COMMAND)?;
        let lo = get(io).ok_or(BAD_COMMAND)?;
        let len = (u16::from_be_bytes([hi, lo]) as usize).clamp(2, MAX_ID);
        out.extend_from_slice(&[hi, lo]);
        while out.len() < len {
            if io.inb(p.status()) & STS_ERROR != 0 {
                break;
            }
            out.push(get(io).ok_or(BAD_COMMAND)?);
        }
        Ok(())
    })();
    terminate(io, p);
    r.map(|_| out)
}

/// The device ID through the best reverse channel the port has: byte
/// mode on a bidirectional port (nibble mode when the peripheral
/// refuses it), else nibble mode.
pub fn device_id<I: Io + ?Sized>(io: &mut I, p: &Port, caps: u8) -> Result<Vec<u8>, u32> {
    if caps & CAP_BYTE != 0 {
        if let Ok(id) = read_device_id(io, p, Reverse::Byte) {
            return Ok(id);
        }
    }
    read_device_id(io, p, Reverse::Nibble)
}

/// The text of a device ID read (after its length bytes), and the value
/// of one of its keys (`MFG`, `MANUFACTURER`, `MDL`, `MODEL`, `CMD`,
/// `COMMAND SET`, ...).
pub fn id_field<'a>(id: &'a [u8], keys: &[&str]) -> Option<&'a [u8]> {
    let text = id.get(2..)?;
    text.split(|&c| c == b';').find_map(|kv| {
        let i = kv.iter().position(|&c| c == b':')?;
        let k = kv[..i].trim_ascii();
        keys.iter().any(|w| k.eq_ignore_ascii_case(w.as_bytes())).then(|| kv[i + 1..].trim_ascii())
    })
}

#[cfg(test)]
mod sim;
#[cfg(test)]
mod tests;
