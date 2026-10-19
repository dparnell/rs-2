//! virtio-console: port 0 as a serial-like character device
//! (`\DEV\VIOCON$`).
//!
//! MULTIPORT is not negotiated, so the device has just port 0 on queues 0
//! (receive) and 1 (transmit). The receive queue is kept full of buffers
//! and polled from the timer tick into a byte buffer that DosRead takes
//! from; DosWrite transmits and waits for the device.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::sync::{Mutex, SpinLock};
use crate::task::WaitQueue;

const RX_BUFS: usize = 16;
const RX_BUF: usize = 256;
const TX_CHUNK: usize = 4096;
const RX_MAX: usize = 4096;
const TIMEOUT_MS: u64 = 5000;

struct Rx {
    q: Virtqueue,
    mem: Dma,
    data: VecDeque<u8>,
}

struct Tx {
    q: Virtqueue,
    mem: Dma,
    _dev: Device,
}

static RX: SpinLock<Option<Rx>> = SpinLock::new(None);
static TX: Mutex<Option<Tx>> = Mutex::new(None);
static RX_WAIT: WaitQueue = WaitQueue::new();
static PRESENT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn attach(mut dev: Device) {
    if PRESENT.load(core::sync::atomic::Ordering::Relaxed) {
        crate::println!("virtio-console {}: only the first console is used", dev.name());
        return;
    }
    if dev.negotiate(0).is_none() {
        return;
    }
    let (Some(mut rq), Some(tq), Some(rmem), Some(tmem)) =
        (dev.queue(0, RX_BUFS as u16), dev.queue(1, 8), Dma::new(RX_BUFS * RX_BUF), Dma::new(TX_CHUNK))
    else {
        crate::println!("virtio-console {}: cannot set up port 0", dev.name());
        dev.fail();
        return;
    };
    let bufs = RX_BUFS.min(rq.size() as usize);
    for i in 0..bufs {
        let _ = rq.add(&[Buf::into(rmem.addr(i * RX_BUF), RX_BUF)]);
    }
    dev.ready();
    rq.kick();
    crate::println!("virtio-console {}: port 0 is \\DEV\\VIOCON$", dev.name());
    *RX.lock() = Some(Rx { q: rq, mem: rmem, data: VecDeque::with_capacity(RX_MAX) });
    *TX.lock() = Some(Tx { q: tq, mem: tmem, _dev: dev });
    PRESENT.store(true, core::sync::atomic::Ordering::Relaxed);
}

pub fn present() -> bool {
    PRESENT.load(core::sync::atomic::Ordering::Relaxed)
}

/// Move received bytes into the buffer (timer tick).
pub fn poll() {
    if !present() {
        return;
    }
    let got = {
        let Some(mut g) = RX.try_lock() else { return };
        let Some(rx) = g.as_mut() else { return };
        let mut got = false;
        while let Some(u) = rx.q.pop() {
            let off = (u.addr - rx.mem.addr(0)) as usize;
            let n = (u.len as usize).min(RX_BUF);
            for i in 0..n {
                if rx.data.len() < RX_MAX {
                    let b = rx.mem.bytes(off + i, 1)[0];
                    rx.data.push_back(b);
                }
            }
            got |= n > 0;
            let _ = rx.q.add(&[Buf::into(u.addr, RX_BUF)]);
            rx.q.kick();
        }
        got
    };
    if got {
        RX_WAIT.wake_all();
    }
}

fn take(len: usize) -> Vec<u8> {
    let mut g = RX.lock();
    let Some(rx) = g.as_mut() else { return Vec::new() };
    let n = len.min(rx.data.len());
    rx.data.drain(..n).collect()
}

/// DosRead: what has arrived, waiting for at least one byte.
pub fn read(len: usize) -> Vec<u8> {
    if len == 0 || !present() {
        return Vec::new();
    }
    loop {
        let v = take(len);
        if !v.is_empty() {
            return v;
        }
        let ready = || RX.lock().as_ref().is_some_and(|rx| !rx.data.is_empty());
        if !RX_WAIT.wait_until(None, ready) {
            return Vec::new();
        }
    }
}

/// DosWrite: transmit `data`; returns the bytes sent.
pub fn write(data: &[u8]) -> usize {
    let mut g = TX.lock();
    let Some(tx) = g.as_mut() else { return 0 };
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
