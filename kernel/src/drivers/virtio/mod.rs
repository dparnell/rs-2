//! virtio devices (OASIS virtio 1.x) on PCI: the paravirtual hardware of
//! KVM/QEMU, Proxmox, UTM and most cloud hypervisors.
//!
//! `pci` is the transport (modern devices through their vendor
//! capabilities, legacy 0.9.5 devices through I/O BAR 0), `queue` the
//! split virtqueue; the device drivers sit on top: block (`blk`), SCSI
//! host (`scsi`), input (`input`: keyboards, mice and tablets), entropy
//! (`rng`), console (`console`), display (`gpu`, 2D) and network (`net`).
//!
//! Interrupts come through MSI-X (PCI INTx cannot be routed without ACPI
//! _PRT and stays masked): queues set up with `queue_irq` get a vector
//! whose handler signals the queue's [`Event`](crate::msi::Event). Block,
//! SCSI, entropy and console requests sleep on it; the input and console
//! receive queues and the network receive queue run their consumer from
//! the handler (a hook that only uses `try_lock`). Without MSI-X (or with
//! `nomsi`) used rings are polled as before: requests by the requesting
//! thread, yielding the CPU, receive queues from the timer tick
//! ([`tick`]), which then also catches a hook that found its lock busy.
//! The display (`gpu`) is always polled: it flushes from the timer tick.

pub mod blk;
pub mod console;
pub mod gpu;
pub mod input;
pub mod net;
pub mod pci;
pub mod queue;
pub mod rng;
pub mod scsi;

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::pci as bus;
use crate::fs::block::BlockDev;

/// PCI vendor of virtio devices.
pub const VENDOR: u16 = 0x1af4;

// Device status bits.
pub const ACKNOWLEDGE: u8 = 1;
pub const DRIVER: u8 = 2;
pub const DRIVER_OK: u8 = 4;
pub const FEATURES_OK: u8 = 8;
pub const NEEDS_RESET: u8 = 0x40;
pub const FAILED: u8 = 0x80;

/// Feature bits common to all devices.
pub const F_VERSION_1: u64 = 1 << 32;

// Device types.
pub const TYPE_NET: u32 = 1;
pub const TYPE_BLOCK: u32 = 2;
pub const TYPE_CONSOLE: u32 = 3;
pub const TYPE_RNG: u32 = 4;
pub const TYPE_SCSI: u32 = 8;
pub const TYPE_GPU: u32 = 16;
pub const TYPE_INPUT: u32 = 18;

/// Polling interval of the input and console queues, in timer ticks.
const POLL_TICKS: u64 = 4;
/// With interrupts, a slow poll still starts the input devices and is
/// the safety net.
const SLOW_POLL_TICKS: u64 = 100;

/// An input or console receive queue has no interrupt: poll it.
static POLLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// An interrupt hook found its lock taken: poll at the next tick.
static RETRY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// A receive queue without an interrupt was set up.
pub(crate) fn needs_polling() {
    POLLED.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// A hook could not get its lock: retry from the tick.
pub(crate) fn retry() {
    RETRY.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// The device type of a virtio PCI function, if it is one: modern IDs are
/// 0x1040 + type, transitional ones (0x1000-0x103f) carry the type in the
/// subsystem ID.
fn device_type(d: &bus::Device) -> Option<u32> {
    if d.vendor != VENDOR {
        return None;
    }
    match d.device {
        0x1040..=0x107f => Some((d.device - 0x1040) as u32),
        0x1000..=0x103f => Some(d.read(0x2c) >> 16),
        _ => None,
    }
}

fn type_name(t: u32) -> &'static str {
    match t {
        TYPE_NET => "network",
        TYPE_BLOCK => "block",
        TYPE_CONSOLE => "console",
        TYPE_RNG => "entropy",
        TYPE_SCSI => "SCSI host",
        TYPE_GPU => "GPU",
        TYPE_INPUT => "input",
        5 => "memory balloon",
        9 => "9P transport",
        _ => "unknown",
    }
}

/// Storage found by [`probe`]: hard disks and optical drives.
#[derive(Default)]
pub struct Found {
    pub disks: Vec<Arc<dyn BlockDev>>,
    pub optical: Vec<Arc<dyn BlockDev>>,
}

/// Set up every virtio function among `devs`. Disks and optical drives are
/// returned for the drive letter assignment; input, entropy and console
/// devices register themselves.
pub fn probe(devs: &[bus::Device]) -> Found {
    let mut found = Found::default();
    for d in devs {
        let Some(t) = device_type(d) else { continue };
        let Some(dev) = pci::Device::new(d) else {
            crate::println!("virtio: {:02x}:{:02x}.{}: {} device without a usable transport", d.bus, d.dev, d.func, type_name(t));
            continue;
        };
        crate::println!("virtio: {}: {} device ({})", dev.name(), type_name(t), if dev.is_legacy() { "legacy" } else { "virtio 1.x" });
        match t {
            TYPE_BLOCK => found.disks.extend(blk::attach(dev)),
            TYPE_SCSI => {
                for (unit, optical) in scsi::attach(dev) {
                    if optical { found.optical.push(unit) } else { found.disks.push(unit) }
                }
            }
            TYPE_INPUT => input::attach(dev),
            TYPE_RNG => rng::attach(dev),
            TYPE_CONSOLE => console::attach(dev),
            TYPE_GPU => gpu::attach(dev),
            TYPE_NET => net::attach(dev),
            _ => crate::println!("virtio: {}: no driver", dev.name()),
        }
    }
    found
}

/// Periodic work from the timer interrupt: input events and console input.
pub fn tick() {
    use core::sync::atomic::Ordering::Relaxed;
    let every = if POLLED.load(Relaxed) { POLL_TICKS } else { SLOW_POLL_TICKS };
    if crate::irq::ticks() % every != 0 && !RETRY.swap(false, Relaxed) {
        return;
    }
    input::poll();
    console::poll();
}

/// Poll `cond` for up to `ms` milliseconds, yielding the CPU. Before the
/// timer runs (during boot) time is estimated from the polls, each of
/// which includes `pace`, an uncached device register read.
pub fn wait(ms: u64, pace: impl FnMut(), cond: impl FnMut() -> bool) -> bool {
    crate::msi::poll(ms, pace, cond)
}
