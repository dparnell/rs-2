//! virtio-input: keyboards, mice and tablets (absolute pointing devices).
//!
//! The device sends Linux evdev events (type, code, value) on its event
//! queue, which is kept full of 8-byte buffers and drained from its
//! MSI-X interrupt (polled from the timer tick without one). Key events become set-1 scancodes for `kbd::scancode`, as USB
//! keyboards do; relative motion goes to `mouse::motion` and absolute
//! positions (tablets: exact pointer placement in a VM, without grabbing
//! the host mouse) to `mouse::motion_abs`, scaled by the axis ranges the
//! device reports (ABS_INFO). Motion and buttons are collected until
//! SYN_REPORT, so one report is one pointer event. The status queue (LEDs)
//! is not used.
//!
//! A device is started (DRIVER_OK) on the first poll, after the PS/2
//! devices are enabled: QEMU sends pointer events to the pointing device
//! activated last, and only routes the host's absolute pointer to a
//! tablet if that is the one.

use alloc::string::String;
use alloc::vec::Vec;

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::sync::SpinLock;

// Configuration: select, subsel, size, then the data union at 8.
const CFG_SELECT: usize = 0;
const CFG_SUBSEL: usize = 1;
const CFG_SIZE: usize = 2;
const CFG_DATA: usize = 8;
const CFG_ID_NAME: u8 = 0x01;
const CFG_EV_BITS: u8 = 0x11;
const CFG_ABS_INFO: u8 = 0x12;

// Event types and codes.
const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const KEY_A: u16 = 30;
const KEY_LEFTCTRL: u16 = 29;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_PAUSE: u16 = 119;

/// Event buffers kept on the queue.
const EVENTS: u16 = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Keyboard,
    Mouse,
    Tablet,
    Other,
}

struct Dev {
    name: String,
    kind: Kind,
    q: Virtqueue,
    mem: Dma,
    /// ABS_X and ABS_Y ranges (min, max).
    range: [(i32, i32); 2],
    buttons: u8,
    dx: i32,
    dy: i32,
    x: i32,
    y: i32,
    rel: bool,
    abs: bool,
    buttons_changed: bool,
    /// Ctrl keys held (bit 0 left, bit 1 right), for Ctrl+Pause.
    ctrl: u8,
    seen: bool,
    /// DRIVER_OK has been set (on the first poll).
    started: bool,
    dev: Device,
}

static DEVS: SpinLock<Vec<Dev>> = SpinLock::new(Vec::new());

/// What a batch of events asks for, done after the device lock is gone.
#[derive(Clone, Copy)]
enum Action {
    Scancode(u8),
    Rel(u8, i32, i32),
    Abs(u8, u32, u32, u32, u32),
}

struct Actions {
    v: [Action; 64],
    n: usize,
}

impl Actions {
    fn push(&mut self, a: Action) {
        if self.n < self.v.len() {
            self.v[self.n] = a;
            self.n += 1;
        }
    }

    fn key(&mut self, sc: u16, release: bool) {
        if sc & 0x100 != 0 {
            self.push(Action::Scancode(0xe0));
        }
        self.push(Action::Scancode(sc as u8 | if release { 0x80 } else { 0 }));
    }

    fn run(&self) {
        for a in &self.v[..self.n] {
            match *a {
                Action::Scancode(b) => crate::kbd::scancode(b),
                Action::Rel(b, dx, dy) => crate::mouse::motion(b, dx, dy),
                Action::Abs(b, x, y, mx, my) => crate::mouse::motion_abs(b, x, y, mx, my),
            }
        }
    }
}

/// Set-1 scancode of a Linux key code; bit 8 = 0xE0 prefix.
fn key_scancode(code: u16) -> Option<u16> {
    const E: u16 = 0x100;
    Some(match code {
        // KEY_ESC..KEY_KPDOT are the set-1 codes themselves.
        1..=83 => code,
        86 => 0x56, // 102nd key
        87 => 0x57, // F11
        88 => 0x58, // F12
        96 => E | 0x1c, // keypad Enter
        97 => E | 0x1d, // right Ctrl
        98 => E | 0x35, // keypad /
        99 => E | 0x37, // SysRq / Print Screen
        100 => E | 0x38, // right Alt
        102 => E | 0x47, // Home
        103 => E | 0x48, // Up
        104 => E | 0x49, // Page Up
        105 => E | 0x4b, // Left
        106 => E | 0x4d, // Right
        107 => E | 0x4f, // End
        108 => E | 0x50, // Down
        109 => E | 0x51, // Page Down
        110 => E | 0x52, // Insert
        111 => E | 0x53, // Delete
        125 => E | 0x5b, // left GUI
        126 => E | 0x5c, // right GUI
        127 => E | 0x5d, // Application
        _ => return None,
    })
}

impl Dev {
    fn event(&mut self, typ: u16, code: u16, value: i32, acts: &mut Actions) {
        match typ {
            EV_KEY => match code {
                BTN_LEFT | BTN_RIGHT | BTN_MIDDLE => {
                    let bit = match code {
                        BTN_LEFT => 1,
                        BTN_RIGHT => 2,
                        _ => 4,
                    };
                    let old = self.buttons;
                    if value != 0 { self.buttons |= bit } else { self.buttons &= !bit }
                    self.buttons_changed |= old != self.buttons;
                }
                KEY_PAUSE => {
                    // Only Ctrl+Pause (Break) means anything to the console.
                    if self.ctrl != 0 {
                        acts.key(0x146, value == 0);
                    }
                }
                _ => {
                    let bit = match code {
                        KEY_LEFTCTRL => 1,
                        KEY_RIGHTCTRL => 2,
                        _ => 0,
                    };
                    if value != 0 { self.ctrl |= bit } else { self.ctrl &= !bit }
                    // Value 2 is autorepeat: a make code again, as PS/2 sends.
                    if let Some(sc) = key_scancode(code) {
                        acts.key(sc, value == 0);
                    }
                }
            },
            EV_REL => match code {
                REL_X => {
                    self.dx += value;
                    self.rel = true;
                }
                REL_Y => {
                    self.dy += value;
                    self.rel = true;
                }
                _ => {}
            },
            EV_ABS => match code {
                ABS_X => {
                    self.x = value;
                    self.abs = true;
                }
                ABS_Y => {
                    self.y = value;
                    self.abs = true;
                }
                _ => {}
            },
            EV_SYN if code == SYN_REPORT => self.report(acts),
            _ => {}
        }
    }

    /// End of a report: one pointer event for its motion and buttons.
    fn report(&mut self, acts: &mut Actions) {
        if self.abs || (self.buttons_changed && self.kind == Kind::Tablet) {
            let axis = |v: i32, (min, max): (i32, i32)| ((v.clamp(min, max) - min) as u32, (max - min).max(1) as u32);
            let (x, mx) = axis(self.x, self.range[0]);
            let (y, my) = axis(self.y, self.range[1]);
            acts.push(Action::Abs(self.buttons, x, y, mx, my));
        } else if self.rel || self.buttons_changed {
            acts.push(Action::Rel(self.buttons, self.dx, self.dy));
        }
        self.dx = 0;
        self.dy = 0;
        self.rel = false;
        self.abs = false;
        self.buttons_changed = false;
    }

    /// Take the events the device has delivered and give the buffers back.
    fn drain(&mut self, acts: &mut Actions) {
        if !self.started {
            self.started = true;
            self.dev.ready();
            self.q.kick();
            return;
        }
        let mut n = 0;
        while let Some(u) = self.q.pop() {
            let off = (u.addr - self.mem.addr(0)) as usize;
            if u.len >= 8 && off + 8 <= self.mem_len() {
                let b = self.mem.bytes(off, 8);
                let typ = u16::from_le_bytes([b[0], b[1]]);
                let code = u16::from_le_bytes([b[2], b[3]]);
                let value = i32::from_le_bytes([b[4], b[5], b[6], b[7]]);
                self.event(typ, code, value, acts);
            }
            let _ = self.q.add(&[Buf::into(u.addr, 8)]);
            n += 1;
            if n >= EVENTS as usize {
                break;
            }
        }
        if n > 0 {
            self.q.kick();
            if !self.seen {
                self.seen = true;
                crate::trace!("virtio-input: {}: first events", self.name);
            }
        }
    }

    fn mem_len(&self) -> usize {
        self.q.size() as usize * 8
    }
}

/// Query a configuration item; returns its bytes.
fn config(dev: &Device, select: u8, subsel: u8) -> Vec<u8> {
    dev.cfg_write8(CFG_SELECT, select);
    dev.cfg_write8(CFG_SUBSEL, subsel);
    let size = (dev.cfg8(CFG_SIZE) as usize).min(128);
    let mut v = alloc::vec![0u8; size];
    dev.cfg_read(CFG_DATA, &mut v);
    v
}

fn has_bit(bits: &[u8], n: u16) -> bool {
    bits.get(n as usize / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

/// Range of an absolute axis.
fn abs_range(dev: &Device, axis: u8) -> (i32, i32) {
    let v = config(dev, CFG_ABS_INFO, axis);
    if v.len() < 8 {
        return (0, 0x7fff);
    }
    let min = i32::from_le_bytes([v[0], v[1], v[2], v[3]]);
    let max = i32::from_le_bytes([v[4], v[5], v[6], v[7]]);
    if max > min { (min, max) } else { (0, 0x7fff) }
}

/// Set up a virtio-input device.
pub fn attach(mut dev: Device) {
    if dev.negotiate(0).is_none() {
        return;
    }
    let name: String = config(&dev, CFG_ID_NAME, 0).iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
    let kind = if has_bit(&config(&dev, CFG_EV_BITS, EV_ABS as u8), ABS_X) {
        Kind::Tablet
    } else if has_bit(&config(&dev, CFG_EV_BITS, EV_REL as u8), REL_X) {
        Kind::Mouse
    } else if has_bit(&config(&dev, CFG_EV_BITS, EV_KEY as u8), KEY_A) {
        Kind::Keyboard
    } else {
        Kind::Other
    };
    let range = if kind == Kind::Tablet { [abs_range(&dev, ABS_X as u8), abs_range(&dev, ABS_Y as u8)] } else { [(0, 0x7fff); 2] };
    let Some(mut q) = dev.queue_irq(0, EVENTS) else {
        crate::println!("virtio-input {}: no event queue", dev.name());
        dev.fail();
        return;
    };
    let Some(mem) = Dma::new(q.size() as usize * 8) else {
        dev.fail();
        return;
    };
    for i in 0..q.size() as usize {
        let _ = q.add(&[Buf::into(mem.addr(i * 8), 8)]);
    }
    match q.event() {
        Some(e) => e.set_hook(poll),
        None => super::needs_polling(),
    }
    match kind {
        Kind::Tablet => crate::println!(
            "virtio-input {}: {name}, tablet (x {}..{}, y {}..{})",
            dev.name(),
            range[0].0,
            range[0].1,
            range[1].0,
            range[1].1
        ),
        _ => crate::println!("virtio-input {}: {name}, {kind:?}", dev.name()),
    }
    let d = Dev {
        name,
        kind,
        q,
        mem,
        range,
        buttons: 0,
        dx: 0,
        dy: 0,
        x: 0,
        y: 0,
        rel: false,
        abs: false,
        buttons_changed: false,
        ctrl: 0,
        seen: false,
        started: false,
        dev,
    };
    DEVS.lock().push(d);
}

/// Process delivered events (timer tick).
pub fn poll() {
    let mut acts = Actions { v: [Action::Scancode(0); 64], n: 0 };
    {
        let Some(mut devs) = DEVS.try_lock() else {
            super::retry();
            return;
        };
        if devs.is_empty() {
            return;
        }
        for d in devs.iter_mut() {
            d.drain(&mut acts);
        }
    }
    acts.run();
}

/// Whether a virtio mouse or tablet is attached.
pub fn pointer_present() -> bool {
    DEVS.lock().iter().any(|d| matches!(d.kind, Kind::Mouse | Kind::Tablet))
}
