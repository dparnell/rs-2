//! virtio-rng: the host's entropy source.
//!
//! One request queue: the driver offers a buffer, the device fills it
//! with random bytes (the used length says how many). Read through
//! `\DEV\RANDOM$` (`chardev`).

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::sync::Mutex;

const CHUNK: usize = 4096;
const TIMEOUT_MS: u64 = 5000;

struct Rng {
    q: Virtqueue,
    mem: Dma,
    _dev: Device,
}

static RNG: Mutex<Option<Rng>> = Mutex::new(None);
static PRESENT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn attach(mut dev: Device) {
    if dev.negotiate(0).is_none() {
        return;
    }
    let (Some(q), Some(mem)) = (dev.queue_irq(0, 8), Dma::new(CHUNK)) else {
        crate::println!("virtio-rng {}: cannot set up the request queue", dev.name());
        dev.fail();
        return;
    };
    dev.ready();
    crate::println!("virtio-rng {}: entropy source", dev.name());
    *RNG.lock() = Some(Rng { q, mem, _dev: dev });
    PRESENT.store(true, core::sync::atomic::Ordering::Relaxed);
}

pub fn present() -> bool {
    PRESENT.load(core::sync::atomic::Ordering::Relaxed)
}

/// Fill `buf` with random bytes from the device; returns how many it gave
/// (0 without a device). Thread context: waits for the device.
pub fn fill(buf: &mut [u8]) -> usize {
    let mut g = RNG.lock();
    let Some(r) = g.as_mut() else { return 0 };
    let mut done = 0;
    let mut empty = 0;
    while done < buf.len() && empty < 4 {
        let n = (buf.len() - done).min(CHUNK);
        let Some(got) = r.q.call(&[Buf::into(r.mem.addr(0), n)], TIMEOUT_MS) else { break };
        let got = (got as usize).min(n);
        if got == 0 {
            empty += 1;
            continue;
        }
        buf[done..done + got].copy_from_slice(r.mem.bytes(0, got));
        done += got;
    }
    done
}
