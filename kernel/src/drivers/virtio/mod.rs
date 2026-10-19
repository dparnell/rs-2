//! virtio devices (OASIS virtio 1.x) on PCI: the paravirtual hardware of
//! KVM/QEMU, Proxmox, UTM and most cloud hypervisors.
//!
//! `pci` is the transport (modern devices through their vendor
//! capabilities, legacy 0.9.5 devices through I/O BAR 0), `queue` the
//! split virtqueue; the device drivers sit on top: block (`blk`), SCSI
//! host (`scsi`), input (`input`: keyboards, mice and tablets), entropy
//! (`rng`), console (`console`) and display (`gpu`, 2D).
//!
//! There are no interrupts yet (no MSI-X, and PCI INTx cannot be routed
//! without ACPI _PRT): INTx is masked, the used rings are polled. Block and
//! SCSI requests are waited for by the requesting thread, yielding the
//! CPU; input and console receive queues are polled from the timer tick
//! ([`tick`]), which only uses spin locks with `try_lock`. Queues keep
//! their interrupt suppressed (VIRTQ_AVAIL_F_NO_INTERRUPT); adding MSI-X
//! means assigning vectors in `pci` and waking the waiters from the
//! handler instead of polling.

pub mod blk;
pub mod console;
pub mod gpu;
pub mod input;
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
            _ => crate::println!("virtio: {}: no driver", dev.name()),
        }
    }
    found
}

/// Periodic work from the timer interrupt: input events and console input.
pub fn tick() {
    if crate::irq::ticks() % POLL_TICKS != 0 {
        return;
    }
    input::poll();
    console::poll();
}

/// Poll `cond` for up to `ms` milliseconds, yielding the CPU. Before the
/// timer runs (during boot) time is estimated from the polls, each of
/// which includes `pace`, an uncached device register read.
pub fn wait(ms: u64, mut pace: impl FnMut(), mut cond: impl FnMut() -> bool) -> bool {
    let t0 = crate::irq::ticks();
    let limit = ms * crate::irq::HZ / 1000;
    let mut polls = 0u64;
    loop {
        if cond() {
            return true;
        }
        pace();
        polls += 1;
        let now = crate::irq::ticks();
        let expired = if now != t0 { now - t0 > limit } else { polls > ms * 1000 };
        if expired {
            return cond();
        }
        if polls > 100 {
            crate::task::sched::yield_now();
        }
        core::hint::spin_loop();
    }
}
