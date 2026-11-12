//! virtio-console (virtio-serial): its ports as serial-like character
//! devices.
//!
//! With MULTIPORT a device has up to `max_nr_ports` ports, which the host
//! adds, names and opens through control messages on queues 2 (receive)
//! and 3 (transmit); port n uses queues 0 and 1 (n = 0) or 2n+2 and 2n+3.
//! Every device present is used (Proxmox puts the SPICE agent's port and
//! the QEMU guest agent's on two controllers). Ports are found by name:
//!
//! - `\DEV\VIOCON$`: the first console port (port 0 without MULTIPORT);
//! - `\DEV\VDAGENT$`: `com.redhat.spice.0`, the SPICE agent's port
//!   (QEMU's qemu-vdagent, SPICE, UTM, Proxmox, libvirt);
//! - `\DEV\QEMUGA$`: `org.qemu.guest_agent.0`, the QEMU guest agent's.
//!
//! Opening a named port tells the host the guest side is open (closed and
//! reopened, so that the host end starts afresh: QEMU's vdagent resets).
//! Receive queues are kept full of buffers and drained into a byte buffer
//! that DosRead takes from; while that is full the device keeps the data
//! (nothing is dropped). DosWrite transmits and waits for the device.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::sync::{Mutex, SpinLock};
use crate::task::WaitQueue;

const F_MULTIPORT: u64 = 1 << 1;

/// Ports set up per device.
const MAX_PORTS: u32 = 8;
const RX_BUFS: usize = 16;
const RX_BUF: usize = 256;
const TX_CHUNK: usize = 4096;
const RX_MAX: usize = 64 * 1024;
const TIMEOUT_MS: u64 = 5000;
const CTL_BUFS: usize = 8;
const CTL_BUF: usize = 256;
const CTL_TX_SLOTS: usize = 16;

// Control events.
const DEVICE_READY: u16 = 0;
const DEVICE_ADD: u16 = 1;
const DEVICE_REMOVE: u16 = 2;
const PORT_READY: u16 = 3;
const CONSOLE_PORT: u16 = 4;
const PORT_OPEN: u16 = 6;
const PORT_NAME: u16 = 7;

/// Which port a device name means (`crate::fs::CharDev::VioPort`).
pub const CONSOLE: u8 = 0;
pub const SPICE: u8 = 1;
pub const QEMU_GA: u8 = 2;

fn kind_name(kind: u8) -> &'static str {
    match kind {
        SPICE => "com.redhat.spice.0",
        QEMU_GA => "org.qemu.guest_agent.0",
        _ => "",
    }
}

struct Port {
    dev: u8,
    id: u32,
    q: Virtqueue,
    mem: Dma,
    data: VecDeque<u8>,
    /// The host has added it (always, without MULTIPORT).
    added: bool,
    console: bool,
    name: String,
    host_open: bool,
}

struct Ctl {
    rx: Virtqueue,
    rx_mem: Dma,
    tx: Virtqueue,
    tx_mem: Dma,
    tx_next: usize,
}

struct Rx {
    ports: Vec<Port>,
    /// Control queues per device (None without MULTIPORT).
    ctls: Vec<Option<Ctl>>,
}

struct TxPort {
    dev: u8,
    id: u32,
    q: Virtqueue,
    mem: Dma,
}

struct Tx {
    ports: Vec<TxPort>,
    _devs: Vec<Device>,
}

static RX: SpinLock<Rx> = SpinLock::new(Rx { ports: Vec::new(), ctls: Vec::new() });
static TX: Mutex<Tx> = Mutex::new(Tx { ports: Vec::new(), _devs: Vec::new() });
static RX_WAIT: WaitQueue = WaitQueue::new();
static PRESENT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn rx_queue(id: u32) -> u16 {
    if id == 0 { 0 } else { (2 * id + 2) as u16 }
}

pub fn attach(mut dev: Device) {
    let devno = {
        let rx = RX.lock();
        rx.ctls.len() as u8
    };
    let Some(_) = dev.negotiate(F_MULTIPORT) else { return };
    let multi = dev.has(F_MULTIPORT);
    let nports = if multi { dev.cfg32(4).clamp(1, MAX_PORTS) } else { 1 };

    let mut rxs = Vec::new();
    let mut txs = Vec::new();
    let mut polled = false;
    for id in 0..nports {
        let qi = rx_queue(id);
        let (Some(mut rq), Some(tq), Some(rmem), Some(tmem)) =
            (dev.queue_irq(qi, RX_BUFS as u16), dev.queue(qi + 1, 8), Dma::new(RX_BUFS * RX_BUF), Dma::new(TX_CHUNK))
        else {
            crate::println!("virtio-console {}: cannot set up port {id}", dev.name());
            if id == 0 {
                dev.fail();
                return;
            }
            break;
        };
        let bufs = RX_BUFS.min(rq.size() as usize);
        for i in 0..bufs {
            let _ = rq.add(&[Buf::into(rmem.addr(i * RX_BUF), RX_BUF)]);
        }
        match rq.event() {
            Some(e) => e.set_hook(poll),
            None => polled = true,
        }
        rxs.push(Port {
            dev: devno,
            id,
            q: rq,
            mem: rmem,
            data: VecDeque::new(),
            added: !multi,
            console: !multi,
            name: String::new(),
            host_open: !multi,
        });
        txs.push(TxPort { dev: devno, id, q: tq, mem: tmem });
    }
    let ctl = if multi {
        let (Some(mut crq), Some(ctq), Some(crmem), Some(ctmem)) =
            (dev.queue_irq(2, CTL_BUFS as u16), dev.queue(3, CTL_TX_SLOTS as u16), Dma::new(CTL_BUFS * CTL_BUF), Dma::new(CTL_TX_SLOTS * 8))
        else {
            crate::println!("virtio-console {}: cannot set up the control queues", dev.name());
            dev.fail();
            return;
        };
        for i in 0..CTL_BUFS.min(crq.size() as usize) {
            let _ = crq.add(&[Buf::into(crmem.addr(i * CTL_BUF), CTL_BUF)]);
        }
        match crq.event() {
            Some(e) => e.set_hook(poll),
            None => polled = true,
        }
        Some(Ctl { rx: crq, rx_mem: crmem, tx: ctq, tx_mem: ctmem, tx_next: 0 })
    } else {
        None
    };
    if polled {
        super::needs_polling();
    }
    dev.ready();
    for p in &rxs {
        p.q.kick();
    }
    if let Some(c) = &ctl {
        c.rx.kick();
    }
    crate::println!(
        "virtio-console {}: {}",
        dev.name(),
        if multi { alloc::format!("{} ports (multiport)", rxs.len()) } else { String::from("port 0 is \\DEV\\VIOCON$") }
    );
    {
        let mut rx = RX.lock();
        rx.ports.extend(rxs);
        rx.ctls.push(ctl);
        if multi {
            send_ctl(&mut rx, devno, 0, DEVICE_READY, 1);
        }
    }
    {
        let mut tx = TX.lock();
        tx.ports.extend(txs);
        tx._devs.push(dev);
    }
    PRESENT.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// Queue a control message (not waited for: also used from interrupts).
fn send_ctl(rx: &mut Rx, dev: u8, id: u32, event: u16, value: u16) {
    let Some(Some(c)) = rx.ctls.get_mut(dev as usize) else { return };
    while c.tx.pop().is_some() {}
    let slot = c.tx_next % CTL_TX_SLOTS;
    c.tx_next += 1;
    let mut m = [0u8; 8];
    m[0..4].copy_from_slice(&id.to_le_bytes());
    m[4..6].copy_from_slice(&event.to_le_bytes());
    m[6..8].copy_from_slice(&value.to_le_bytes());
    c.tx_mem.write(slot * 8, &m);
    if c.tx.add(&[Buf::out(c.tx_mem.addr(slot * 8), 8)]).is_some() {
        c.tx.kick();
    }
}

pub fn present() -> bool {
    PRESENT.load(core::sync::atomic::Ordering::Relaxed)
}

/// Move received bytes into the port's buffer while it has room.
fn drain(p: &mut Port) -> bool {
    let mut got = false;
    while p.data.len() + RX_BUF <= RX_MAX {
        let Some(u) = p.q.pop() else { break };
        let off = (u.addr - p.mem.addr(0)) as usize;
        let n = (u.len as usize).min(RX_BUF);
        p.data.extend(p.mem.bytes(off, n));
        got |= n > 0;
        let _ = p.q.add(&[Buf::into(u.addr, RX_BUF)]);
        p.q.kick();
    }
    got
}

fn control(rx: &mut Rx, dev: u8) {
    let mut msgs = Vec::new();
    {
        let Some(Some(c)) = rx.ctls.get_mut(dev as usize) else { return };
        while let Some(u) = c.rx.pop() {
            let off = (u.addr - c.rx_mem.addr(0)) as usize;
            let n = (u.len as usize).min(CTL_BUF);
            msgs.push(c.rx_mem.bytes(off, n).to_vec());
            let _ = c.rx.add(&[Buf::into(u.addr, CTL_BUF)]);
            c.rx.kick();
        }
    }
    for m in msgs {
        if m.len() < 8 {
            continue;
        }
        let id = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);
        let event = u16::from_le_bytes([m[4], m[5]]);
        let value = u16::from_le_bytes([m[6], m[7]]);
        let Some(p) = rx.ports.iter_mut().find(|p| p.dev == dev && p.id == id) else {
            if event == DEVICE_ADD {
                // Beyond the ports set up.
                send_ctl(rx, dev, id, PORT_READY, 0);
            }
            continue;
        };
        let reply = match event {
            DEVICE_ADD => {
                p.added = true;
                Some((PORT_READY, 1))
            }
            DEVICE_REMOVE => {
                p.added = false;
                p.host_open = false;
                p.name.clear();
                p.data.clear();
                None
            }
            CONSOLE_PORT => {
                p.console = true;
                // A console is always open on the guest side.
                Some((PORT_OPEN, 1))
            }
            PORT_OPEN => {
                p.host_open = value != 0;
                None
            }
            PORT_NAME => {
                let name = &m[8..];
                let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                p.name = name[..end].iter().map(|&b| b as char).collect();
                crate::println!("virtio-console: port {dev}.{id} is \"{}\"", p.name);
                None
            }
            _ => None,
        };
        if let Some((e, v)) = reply {
            send_ctl(rx, dev, id, e, v);
        }
    }
}

/// Take what has arrived (timer tick and queue interrupts).
pub fn poll() {
    if !present() {
        return;
    }
    let got = {
        let Some(mut rx) = RX.try_lock() else {
            super::retry();
            return;
        };
        for d in 0..rx.ctls.len() {
            control(&mut rx, d as u8);
        }
        let mut got = false;
        for p in rx.ports.iter_mut() {
            got |= drain(p);
        }
        got
    };
    if got {
        RX_WAIT.wake_all();
    }
}

fn find(rx: &Rx, kind: u8) -> Option<usize> {
    rx.ports.iter().position(|p| {
        p.added
            && match kind {
                CONSOLE => p.console,
                k => p.name == kind_name(k),
            }
    })
}

/// Whether the port a device name means is there.
pub fn port_present(kind: u8) -> bool {
    present() && find(&RX.lock(), kind).is_some()
}

/// A named port was opened: tell the host (closed first, so that it
/// starts afresh) and drop what arrived before.
pub fn open(kind: u8) {
    if kind == CONSOLE {
        return;
    }
    let mut rx = RX.lock();
    let Some(i) = find(&rx, kind) else { return };
    let (dev, id) = (rx.ports[i].dev, rx.ports[i].id);
    rx.ports[i].data.clear();
    crate::println!("virtio-console: port {dev}.{id} (\"{}\") opened", kind_name(kind));
    send_ctl(&mut rx, dev, id, PORT_OPEN, 0);
    send_ctl(&mut rx, dev, id, PORT_OPEN, 1);
}

fn take(kind: u8, len: usize) -> Option<Vec<u8>> {
    let mut rx = RX.lock();
    let i = find(&rx, kind)?;
    let p = &mut rx.ports[i];
    let n = len.min(p.data.len());
    let v = p.data.drain(..n).collect();
    drain(p);
    Some(v)
}

/// DosRead: what has arrived, waiting for at least one byte.
pub fn read(kind: u8, len: usize) -> Vec<u8> {
    if len == 0 || !present() {
        return Vec::new();
    }
    loop {
        match take(kind, len) {
            None => return Vec::new(),
            Some(v) if !v.is_empty() => return v,
            Some(_) => {}
        }
        let ready = || {
            let rx = RX.lock();
            find(&rx, kind).is_none_or(|i| !rx.ports[i].data.is_empty() || rx.ports[i].q.has_used())
        };
        if !RX_WAIT.wait_until(None, ready) {
            return Vec::new();
        }
    }
}

/// DosWrite: transmit `data`; returns the bytes sent.
pub fn write(kind: u8, data: &[u8]) -> usize {
    let (dev, id) = {
        let rx = RX.lock();
        let Some(i) = find(&rx, kind) else { return 0 };
        (rx.ports[i].dev, rx.ports[i].id)
    };
    let mut g = TX.lock();
    let Some(tx) = g.ports.iter_mut().find(|t| t.dev == dev && t.id == id) else { return 0 };
    let mut done = 0;
    for chunk in data.chunks(TX_CHUNK) {
        tx.mem.write(0, chunk);
        if tx.q.call(&[Buf::out(tx.mem.addr(0), chunk.len())], TIMEOUT_MS).is_none() {
            break;
        }
        done += chunk.len();
    }
    done
}
