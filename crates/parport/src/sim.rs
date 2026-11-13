//! A simulated parallel port for the host tests: the chip (SPP latch,
//! PS/2 direction bit, an ECP chip's ECR and 16-byte FIFO, EPP data and
//! address cycles with the timeout bit) and a peripheral behind it (a
//! compatibility-mode printer, optionally an IEEE 1284 one: negotiation,
//! nibble and byte mode reverse channels with a device ID, ECP forward
//! transfers, EPP cycles, termination). Time is simulated: every port
//! access takes a microsecond, a pause 100 us, during which the chip
//! moves a byte from its FIFO to the peripheral.

use std::collections::VecDeque;

use crate::*;

const FIFO_SIZE: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    Compat,
    /// Event 2 given; waiting for the strobe (3) and its release (4).
    Neg { strobed: bool },
    /// The request refused: waiting for termination.
    Refused,
    /// Reverse idle (nibble or byte mode), a device ID to send.
    Rev { byte: bool, half: u8, busy: bool },
    /// ECP mode, waiting for event 30.
    EcpSetup,
    EcpFwd,
    /// Termination: nAck low (event 24), waiting for nAutoFd low.
    Term,
}

/// The peripheral.
pub struct Dev {
    pub ieee1284: bool,
    pub ecp: bool,
    pub byte: bool,
    pub epp: bool,
    /// The whole device ID, length bytes included.
    pub id: Vec<u8>,
    pub paper_out: bool,
    pub printed: Vec<u8>,
    pub ecp_data: Vec<u8>,
    pub epp_data: Vec<u8>,
    /// Negotiations seen (extensibility bytes).
    pub requests: Vec<u8>,
    st: St,
    pos: usize,
    n_ack: bool,
    busy: bool,
    perror: bool,
    select: bool,
    n_fault: bool,
    /// Byte mode: the byte on the data lines.
    drive: Option<u8>,
}

impl Dev {
    /// A printer that knows only compatibility mode (QEMU's port has
    /// nothing else behind it).
    pub fn printer() -> Dev {
        let mut d = Dev {
            ieee1284: false,
            ecp: false,
            byte: false,
            epp: false,
            id: Vec::new(),
            paper_out: false,
            printed: Vec::new(),
            ecp_data: Vec::new(),
            epp_data: Vec::new(),
            requests: Vec::new(),
            st: St::Compat,
            pos: 0,
            n_ack: true,
            busy: false,
            perror: false,
            select: true,
            n_fault: true,
            drive: None,
        };
        d.compat_lines();
        d
    }

    /// An IEEE 1284 printer with device ID `text` (the length counts the
    /// two length bytes).
    pub fn ieee1284(text: &str, ecp: bool, byte: bool) -> Dev {
        let mut d = Dev::printer();
        d.ieee1284 = true;
        d.ecp = ecp;
        d.byte = byte;
        d.id = ((text.len() + 2) as u16).to_be_bytes().to_vec();
        d.id.extend_from_slice(text.as_bytes());
        d
    }

    fn compat_lines(&mut self) {
        self.n_ack = true;
        self.busy = self.paper_out;
        self.perror = self.paper_out;
        self.select = true;
        self.n_fault = true;
        self.drive = None;
    }

    fn status(&self) -> u8 {
        (self.n_fault as u8) << 3 | (self.select as u8) << 4 | (self.perror as u8) << 5 | (self.n_ack as u8) << 6 | (!self.busy as u8) << 7
    }

    fn ready(&self) -> bool {
        self.st == St::Compat && !self.paper_out
    }

    /// Out of paper: busy, PError high (PRINT01's "out of paper").
    pub fn set_paper_out(&mut self, v: bool) {
        self.paper_out = v;
        if self.st == St::Compat {
            self.compat_lines();
        }
    }

    pub fn in_compat(&self) -> bool {
        self.st == St::Compat
    }

    fn data_avail(&self) -> bool {
        self.pos < self.id.len()
    }

    /// The host changed the control lines.
    fn control(&mut self, old: u8, new: u8, latch: u8) {
        let strobe = |c: u8| c & CTR_STROBE != 0;
        let autofd = |c: u8| c & CTR_AUTOFD != 0;
        let selectin = |c: u8| c & CTR_SELECT != 0;
        // Events 22-23: nSelectIn low with nAutoFd high ends any IEEE
        // 1284 mode; event 24 nAck low.
        if !matches!(self.st, St::Compat | St::Term) && selectin(new) && !autofd(new) {
            self.st = St::Term;
            self.drive = None;
            self.n_ack = false;
            return;
        }
        match self.st {
            St::Compat => {
                if self.ieee1284 && !selectin(new) && autofd(new) && !(!selectin(old) && autofd(old)) {
                    // Event 1 seen: event 2.
                    self.requests.push(latch);
                    self.st = St::Neg { strobed: false };
                    self.n_ack = false;
                    self.perror = true;
                    self.select = true;
                    self.n_fault = true;
                } else if strobe(new) && !strobe(old) && self.ready() && selectin(new) {
                    self.printed.push(latch);
                }
            }
            St::Neg { strobed } => {
                if strobe(new) {
                    self.st = St::Neg { strobed: true };
                } else if strobed && !autofd(new) {
                    // Events 5-6.
                    let x = *self.requests.last().unwrap();
                    let ok = match x {
                        0x00 => true,
                        0x04 => !self.id.is_empty(),
                        0x01 => self.byte,
                        0x05 => self.byte && !self.id.is_empty(),
                        0x10 => self.ecp,
                        _ => false,
                    };
                    self.perror = false;
                    self.select = if x == 0 { !ok } else { ok };
                    self.pos = 0;
                    self.st = if !ok {
                        St::Refused
                    } else if x == 0x10 {
                        St::EcpSetup
                    } else {
                        St::Rev { byte: x & 1 != 0, half: 0, busy: false }
                    };
                    self.n_fault = !(ok && x & 4 != 0 && self.data_avail());
                    self.n_ack = true;
                }
            }
            St::Refused | St::EcpFwd => {}
            St::EcpSetup => {
                if autofd(new) {
                    // Event 31.
                    self.perror = true;
                    self.st = St::EcpFwd;
                }
            }
            St::Rev { byte, half, busy } => {
                if !busy && autofd(new) && !autofd(old) && self.data_avail() {
                    // Event 7 -> 9: the data and PtrClk low.
                    let b = self.id[self.pos];
                    if byte {
                        self.drive = Some(b);
                    } else {
                        let nib = if half == 0 { b & 15 } else { b >> 4 };
                        self.n_fault = nib & 1 != 0;
                        self.select = nib & 2 != 0;
                        self.perror = nib & 4 != 0;
                        self.busy = nib & 8 != 0;
                    }
                    self.n_ack = false;
                    self.st = St::Rev { byte, half, busy: true };
                } else if busy && !autofd(new) && autofd(old) {
                    // Event 10 -> 11.
                    self.n_ack = true;
                    if byte {
                        // Held until the host's HostClk pulse (16-17).
                        self.st = St::Rev { byte, half: 1, busy: false };
                    } else if half == 0 {
                        self.st = St::Rev { byte, half: 1, busy: false };
                    } else {
                        self.next_byte(byte);
                    }
                } else if byte && half == 1 && strobe(new) && !strobe(old) {
                    self.drive = None;
                    self.next_byte(byte);
                }
            }
            St::Term => {
                if autofd(new) {
                    // Events 25-27, back to compatibility mode.
                    self.st = St::Compat;
                    self.compat_lines();
                }
            }
        }
    }

    fn next_byte(&mut self, byte: bool) {
        self.pos += 1;
        self.st = St::Rev { byte, half: 0, busy: false };
        self.busy = false;
        self.perror = true;
        self.select = true;
        self.n_fault = !self.data_avail();
    }
}

/// The chip and its peripheral.
pub struct Sim {
    pub base: u16,
    pub present: bool,
    pub has_ecr: bool,
    /// Without an ECR, base+402h mirrors the control register (as some
    /// chips decode only ten address lines).
    pub ecr_mirror: bool,
    pub has_epp: bool,
    pub bidir: bool,
    pub dev: Dev,
    pub t_us: u64,
    latch: u8,
    ctr: u8,
    ecr_mode: u8,
    ecr_bits: u8,
    fifo: VecDeque<u8>,
    epp_timeout: bool,
}

impl Sim {
    /// A plain SPP port at 378h (QEMU's).
    pub fn spp(dev: Dev) -> Sim {
        Sim {
            base: 0x378,
            present: true,
            has_ecr: false,
            ecr_mirror: false,
            has_epp: false,
            bidir: false,
            dev,
            t_us: 0,
            latch: 0,
            ctr: CTR_IDLE,
            ecr_mode: ECR_PS2,
            ecr_bits: ECR_NOINTR,
            fifo: VecDeque::new(),
            epp_timeout: false,
        }
    }

    /// A Super I/O chip with ECP, EPP and PS/2 modes.
    pub fn ecp(dev: Dev) -> Sim {
        Sim { has_ecr: true, has_epp: true, bidir: true, ..Sim::spp(dev) }
    }

    pub fn port(&self) -> Port {
        Port { base: self.base, ecr: if self.has_ecr { self.base + 0x402 } else { 0 } }
    }

    fn epp_on(&self) -> bool {
        self.has_epp && (!self.has_ecr || self.ecr_mode == ECR_EPP)
    }

    /// The chip moves a byte from its FIFO to the peripheral.
    fn step(&mut self) {
        if self.fifo.is_empty() {
            return;
        }
        match self.ecr_mode {
            ECR_FIFO if self.dev.ready() => {
                let b = self.fifo.pop_front().unwrap();
                self.dev.printed.push(b);
            }
            ECR_ECP if self.dev.st == St::EcpFwd => {
                let b = self.fifo.pop_front().unwrap();
                self.dev.ecp_data.push(b);
            }
            _ => {}
        }
    }
}

impl Io for Sim {
    fn inb(&mut self, port: u16) -> u8 {
        self.t_us += 1;
        if !self.present {
            return 0xff;
        }
        let b = self.base;
        match port.wrapping_sub(b) {
            0 => {
                if self.ctr & CTR_DIR != 0 && self.bidir {
                    self.dev.drive.unwrap_or(0xff)
                } else {
                    self.latch
                }
            }
            1 => self.dev.status() | self.epp_timeout as u8,
            2 => self.ctr | 0xc0,
            3 | 4 if self.epp_on() => {
                if self.dev.epp {
                    0
                } else {
                    self.epp_timeout = true;
                    0xff
                }
            }
            0x402 if self.has_ecr => {
                self.ecr_mode | self.ecr_bits | ((self.fifo.len() >= FIFO_SIZE) as u8) << 1 | self.fifo.is_empty() as u8
            }
            0x402 if self.ecr_mirror => self.ctr | 0xc0,
            _ => 0xff,
        }
    }

    fn outb(&mut self, port: u16, v: u8) {
        self.t_us += 1;
        if !self.present {
            return;
        }
        match port.wrapping_sub(self.base) {
            0 => self.latch = v,
            // The timeout bit clears when written with 1.
            1 => {
                if v & STS_EPP_TIMEOUT != 0 {
                    self.epp_timeout = false;
                }
            }
            2 => {
                let old = self.ctr;
                self.ctr = v & 0x3f;
                let latch = self.latch;
                self.dev.control(old, self.ctr, latch);
            }
            3 | 4 if self.epp_on() => {
                if self.dev.epp {
                    if port == self.base + 4 {
                        self.dev.epp_data.push(v);
                    }
                } else {
                    self.epp_timeout = true;
                }
            }
            0x400 if self.has_ecr && matches!(self.ecr_mode, ECR_FIFO | ECR_ECP) => {
                if self.fifo.len() < FIFO_SIZE {
                    self.fifo.push_back(v);
                }
            }
            0x402 if self.has_ecr => {
                let mode = v & 0xe0;
                if mode != self.ecr_mode && matches!(mode, ECR_SPP | ECR_PS2) {
                    self.fifo.clear();
                }
                self.ecr_mode = mode;
                self.ecr_bits = v & 0x1c;
            }
            _ => {}
        }
    }

    fn deadline(&mut self, ms: u64) -> u64 {
        self.t_us + ms * 1000
    }

    fn expired(&mut self, d: u64) -> bool {
        self.t_us >= d
    }

    fn pause(&mut self, d: u64, _mark: u64) -> bool {
        self.t_us = (self.t_us + 100).min(d.max(self.t_us + 1));
        self.step();
        true
    }
}
