//! USB hubs (USB 2.0 chapter 11, USB 3.x chapter 10): the hub descriptor,
//! port status and the connect, debounce and reset sequence of each
//! downstream port.
//!
//! This is the class logic only. The host controller driver runs it: it
//! performs the control transfers [`Hub::poll`] asks for on the hub's
//! default pipe, reports their completion with [`Hub::done`] or
//! [`Hub::failed`], passes on the status-change bitmaps of the hub's
//! interrupt endpoint ([`Hub::changed`]), and creates or removes the
//! devices behind a port on [`Action::Attach`] and [`Action::Detach`].
//!
//! Only one control request is in flight per hub. A port is reset only
//! while no other device anywhere is between reset and Address Device
//! (the caller says so with `may_reset`), because a reset device answers
//! at the default address.

use super::usb::{speed, Setup};

/// Most downstream ports a hub can have (route string nibbles hold 1-15).
pub const MAX_PORTS: u8 = 15;

// Port feature selectors.
const PORT_RESET: u16 = 4;
const PORT_POWER: u16 = 8;
const C_HUB_LOCAL_POWER: u16 = 0;
const C_HUB_OVER_CURRENT: u16 = 1;

// wPortStatus bits.
const ST_CONNECTION: u16 = 1 << 0;
const ST_ENABLE: u16 = 1 << 1;
const ST_LOW_SPEED: u16 = 1 << 9;
const ST_HIGH_SPEED: u16 = 1 << 10;

// wPortChange bits.
const CH_CONNECTION: u16 = 1 << 0;
const CH_RESET: u16 = 1 << 4;
/// USB 3: warm (BH) reset change.
const CH_BH_RESET: u16 = 1 << 5;

/// Feature selector that clears change bit `n` of wPortChange (USB 2 and
/// USB 3 numbering), or 0 if the bit is undefined.
fn change_feature(usb3: bool, n: u32) -> u16 {
    const USB2: [u16; 5] = [16, 17, 18, 19, 20];
    const USB3: [u16; 8] = [16, 0, 0, 19, 20, 29, 25, 26];
    let t: &[u16] = if usb3 { &USB3 } else { &USB2 };
    t.get(n as usize).copied().unwrap_or(0)
}

/// Debounce before a port reset, reset timeout, recovery after a reset
/// and how often a port being reset is polled (ms).
const DEBOUNCE_MS: u64 = 100;
const RESET_TIMEOUT_MS: u64 = 500;
const RECOVERY_MS: u64 = 10;
const RESET_POLL_MS: u64 = 10;

/// The fields of a hub descriptor we use.
#[derive(Clone, Copy, Debug, Default)]
pub struct HubDesc {
    pub ports: u8,
    pub characteristics: u16,
    /// Time from port power on until power is good.
    pub power_on_ms: u64,
}

impl HubDesc {
    /// TT think time (USB 2 high-speed hubs): 0-3 for 8-32 FS bit times.
    pub fn tt_think(&self) -> u8 {
        ((self.characteristics >> 5) & 3) as u8
    }
}

/// Parse a hub descriptor (type 0x29 or, for USB 3 hubs, 0x2a).
pub fn parse_desc(d: &[u8]) -> Option<HubDesc> {
    if d.len() < 7 || !matches!(d[1], super::usb::DESC_HUB | super::usb::DESC_SS_HUB) {
        return None;
    }
    Some(HubDesc {
        ports: d[2].min(MAX_PORTS),
        characteristics: u16::from_le_bytes([d[3], d[4]]),
        power_on_ms: d[5] as u64 * 2,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Empty,
    Debounce(u64),
    /// Reset requested; times out at the deadline.
    Reset(u64),
    /// Reset done, recovery time before the device is addressed.
    Settle(u64),
    /// A device is attached (being enumerated or in use).
    Attached,
    /// Reset failed; waits for a disconnect.
    Failed,
}

#[derive(Clone, Copy, Debug)]
struct Port {
    phase: Phase,
    speed: u8,
    /// Change bits still to acknowledge.
    clear: u16,
    next_poll: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// Waiting for the hub to be configured.
    Idle,
    /// Powering port n.
    PowerOn(u8),
    PowerWait(u64),
    Running,
}

/// The control request in flight.
#[derive(Clone, Copy, Debug)]
enum Op {
    Power(u8),
    Status(u8),
    Clear(u8, u16),
    Reset(u8),
    HubStatus,
    HubClear(u16),
}

/// What the host controller driver should do next for this hub.
#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// Issue this control request to the hub.
    Control(Setup),
    /// A device on `port` was reset and is ready to be addressed.
    Attach { port: u8, speed: u8 },
    /// The device on `port` is gone.
    Detach(u8),
}

pub struct Hub {
    pub desc: HubDesc,
    pub usb3: bool,
    state: State,
    ports: [Port; MAX_PORTS as usize + 1],
    /// Bit n: port n's status must be read; bit 0: the hub's status.
    dirty: u32,
    /// Ports whose device must be detached.
    detach: u32,
    /// Hub change bits to acknowledge.
    hub_clear: u16,
    op: Option<Op>,
}

impl Hub {
    pub fn new(usb3: bool) -> Hub {
        let port = Port { phase: Phase::Empty, speed: 0, clear: 0, next_poll: 0 };
        Hub {
            desc: HubDesc::default(),
            usb3,
            state: State::Idle,
            ports: [port; MAX_PORTS as usize + 1],
            dirty: 0,
            detach: 0,
            hub_clear: 0,
            op: None,
        }
    }

    /// The hub is configured: power its ports.
    pub fn start(&mut self) {
        self.state = State::PowerOn(1);
    }

    /// A port is being reset or recovering: its device is at the default
    /// address.
    pub fn resetting(&self) -> bool {
        self.ports.iter().any(|p| matches!(p.phase, Phase::Reset(_) | Phase::Settle(_)))
    }

    /// Powered and every port's state is known (boot waits for this).
    pub fn settled(&self) -> bool {
        self.state == State::Running
            && self.dirty == 0
            && self.op.is_none()
            && self.ports.iter().all(|p| matches!(p.phase, Phase::Empty | Phase::Attached | Phase::Failed))
    }

    /// Status-change bitmap from the interrupt endpoint.
    pub fn changed(&mut self, bitmap: &[u8]) {
        let mut bits = 0u32;
        for (i, &b) in bitmap.iter().take(4).enumerate() {
            bits |= (b as u32) << (i * 8);
        }
        let mask = (2u32 << self.desc.ports) - 1;
        if self.state == State::Running {
            self.dirty |= bits & mask;
        }
    }

    /// The next thing to do, if any; `may_reset` allows starting a port
    /// reset. Call again after acting on Attach or Detach.
    pub fn poll(&mut self, now: u64, may_reset: bool) -> Option<Action> {
        if self.op.is_some() {
            return None;
        }
        match self.state {
            State::Idle => return None,
            State::PowerOn(p) if p <= self.desc.ports => {
                return self.request(Op::Power(p), Setup::set_port_feature(p, PORT_POWER));
            }
            State::PowerOn(_) => {
                self.state = State::PowerWait(now + self.desc.power_on_ms.max(100));
                return None;
            }
            State::PowerWait(t) if now < t => return None,
            State::PowerWait(_) => {
                self.state = State::Running;
                self.dirty = (2u32 << self.desc.ports) - 2;
            }
            State::Running => {}
        }
        if self.detach != 0 {
            let p = self.detach.trailing_zeros() as u8;
            self.detach &= !(1 << p);
            return Some(Action::Detach(p));
        }
        if self.hub_clear != 0 {
            let f = self.hub_clear.trailing_zeros() as u16;
            return self.request(Op::HubClear(f), Setup::clear_hub_feature(f));
        }
        for p in 1..=self.desc.ports {
            let c = self.ports[p as usize].clear;
            if c != 0 {
                let bit = c.trailing_zeros();
                let f = change_feature(self.usb3, bit);
                if f == 0 {
                    self.ports[p as usize].clear &= !(1 << bit);
                    continue;
                }
                return self.request(Op::Clear(p, 1 << bit), Setup::clear_port_feature(p, f));
            }
        }
        if self.dirty & 1 != 0 {
            return self.request(Op::HubStatus, Setup::hub_status());
        }
        if self.dirty != 0 {
            let p = self.dirty.trailing_zeros() as u8;
            return self.request(Op::Status(p), Setup::port_status(p));
        }
        self.port_timers(now, may_reset)
    }

    fn request(&mut self, op: Op, s: Setup) -> Option<Action> {
        self.op = Some(op);
        Some(Action::Control(s))
    }

    fn port_timers(&mut self, now: u64, may_reset: bool) -> Option<Action> {
        let idle = may_reset && !self.resetting();
        for p in 1..=self.desc.ports {
            let port = &mut self.ports[p as usize];
            match port.phase {
                Phase::Debounce(t) if now >= t && idle => {
                    self.op = Some(Op::Reset(p));
                    return Some(Action::Control(Setup::set_port_feature(p, PORT_RESET)));
                }
                Phase::Reset(deadline) if now >= deadline => {
                    crate::println!("usb hub: port {p} reset timed out");
                    port.phase = Phase::Failed;
                }
                Phase::Reset(_) if now >= port.next_poll => {
                    port.next_poll = now + RESET_POLL_MS;
                    self.op = Some(Op::Status(p));
                    return Some(Action::Control(Setup::port_status(p)));
                }
                Phase::Settle(t) if now >= t => {
                    port.phase = Phase::Attached;
                    return Some(Action::Attach { port: p, speed: port.speed });
                }
                _ => {}
            }
        }
        None
    }

    /// The request in flight completed; `data` is what it returned.
    pub fn done(&mut self, data: &[u8], now: u64) {
        let Some(op) = self.op.take() else { return };
        match op {
            Op::Power(p) => self.state = State::PowerOn(p + 1),
            Op::Status(p) => {
                self.dirty &= !(1 << p);
                if data.len() >= 4 {
                    let status = u16::from_le_bytes([data[0], data[1]]);
                    let change = u16::from_le_bytes([data[2], data[3]]);
                    self.port_status(p, status, change, now);
                }
            }
            Op::Clear(p, bit) => self.ports[p as usize].clear &= !bit,
            Op::Reset(p) => {
                let port = &mut self.ports[p as usize];
                port.phase = Phase::Reset(now + RESET_TIMEOUT_MS);
                port.next_poll = now + RESET_POLL_MS;
            }
            Op::HubStatus => {
                self.dirty &= !1;
                if data.len() >= 4 {
                    let change = u16::from_le_bytes([data[2], data[3]]);
                    self.hub_clear |= change & (1 << C_HUB_LOCAL_POWER | 1 << C_HUB_OVER_CURRENT);
                }
            }
            Op::HubClear(f) => self.hub_clear &= !(1 << f),
        }
    }

    /// The request in flight failed (stalled or timed out).
    pub fn failed(&mut self) {
        let Some(op) = self.op.take() else { return };
        match op {
            Op::Power(p) => self.state = State::PowerOn(p + 1),
            Op::Status(p) => self.dirty &= !(1 << p),
            Op::Clear(p, bit) => self.ports[p as usize].clear &= !bit,
            Op::Reset(p) => {
                crate::println!("usb hub: port {p} reset request failed");
                self.ports[p as usize].phase = Phase::Failed;
            }
            Op::HubStatus => self.dirty &= !1,
            Op::HubClear(f) => self.hub_clear &= !(1 << f),
        }
    }

    fn port_status(&mut self, p: u8, status: u16, change: u16, now: u64) {
        let usb3 = self.usb3;
        let port = &mut self.ports[p as usize];
        port.clear |= change;
        let connected = status & ST_CONNECTION != 0;
        let reconnect = change & CH_CONNECTION != 0;
        match port.phase {
            Phase::Attached if !connected || reconnect => {
                self.detach |= 1 << p;
                port.phase = if connected { Phase::Debounce(now + DEBOUNCE_MS) } else { Phase::Empty };
            }
            Phase::Empty | Phase::Failed if connected && (reconnect || port.phase == Phase::Empty) => {
                port.phase = Phase::Debounce(now + DEBOUNCE_MS);
            }
            Phase::Debounce(_) if connected && reconnect => port.phase = Phase::Debounce(now + DEBOUNCE_MS),
            Phase::Empty | Phase::Failed | Phase::Debounce(_) | Phase::Reset(_) | Phase::Settle(_) if !connected => {
                port.phase = Phase::Empty;
            }
            Phase::Reset(_) => {
                let reset_done = change & (CH_RESET | CH_BH_RESET) != 0;
                if reset_done && status & ST_ENABLE != 0 {
                    port.speed = if usb3 {
                        speed::SUPER
                    } else if status & ST_LOW_SPEED != 0 {
                        speed::LOW
                    } else if status & ST_HIGH_SPEED != 0 {
                        speed::HIGH
                    } else {
                        speed::FULL
                    };
                    port.phase = Phase::Settle(now + RECOVERY_MS);
                } else if reset_done {
                    crate::println!("usb hub: port {p} not enabled after reset");
                    port.phase = Phase::Failed;
                }
            }
            _ => {}
        }
    }
}
