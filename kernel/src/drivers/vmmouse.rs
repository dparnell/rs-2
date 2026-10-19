//! VMware absolute pointer ("vmmouse"), offered by VMware and by QEMU
//! with `vmport=on`: the pointer position comes from the host cursor
//! through the backdoor (`crate::vmware`), so it follows the host's mouse
//! without capturing it.
//!
//! The PS/2 auxiliary device stays enabled: the hypervisor signals new
//! data with (dummy) PS/2 packets on IRQ 12, and on every such interrupt
//! `mouse::irq` drains the vmmouse queue here; PS/2 packets that carry no
//! vmmouse data are still taken as relative motion. Each vmmouse packet
//! is four words: buttons and flags, x, y (0..=0xFFFF across the screen)
//! and the wheel.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::vmware::{self, CMD_ABSPOINTER_COMMAND, CMD_ABSPOINTER_DATA, CMD_ABSPOINTER_STATUS};

// ABSPOINTER_COMMAND arguments.
const CMD_ENABLE: u32 = 0x4541_4552;
const CMD_DISABLE: u32 = 0x0000_00f5;
const CMD_REQUEST_ABSOLUTE: u32 = 0x5342_4152;
/// First word queued after CMD_ENABLE.
const VERSION_ID: u32 = 0x3442_554a;
/// ABSPOINTER_STATUS: error flag in the high word; the low word counts
/// queued words.
const STATUS_ERROR: u32 = 0xffff_0000;
// Packet flags (word 0).
const LEFT: u32 = 0x20;
const RIGHT: u32 = 0x10;
const MIDDLE: u32 = 0x08;
const RELATIVE_PACKET: u32 = 0x0001_0000;
/// Largest coordinate.
const MAX: u32 = 0xffff;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static BUTTONS: AtomicU8 = AtomicU8::new(0);

fn cmd(c: u32, arg: u32) -> u32 {
    vmware::command(c, arg).map_or(STATUS_ERROR, |r| r.eax)
}

/// Enable the device and switch it to absolute mode.
fn enable() -> bool {
    cmd(CMD_ABSPOINTER_COMMAND, CMD_ENABLE);
    let st = cmd(CMD_ABSPOINTER_STATUS, 0);
    if st & STATUS_ERROR == STATUS_ERROR || st & 0xffff == 0 {
        return false;
    }
    if cmd(CMD_ABSPOINTER_DATA, 1) != VERSION_ID {
        cmd(CMD_ABSPOINTER_COMMAND, CMD_DISABLE);
        return false;
    }
    cmd(CMD_ABSPOINTER_COMMAND, CMD_REQUEST_ABSOLUTE);
    true
}

/// Called once the PS/2 mouse is up: use the absolute pointer if the
/// hypervisor has one.
pub fn init() {
    if !vmware::present() || !enable() {
        return;
    }
    ACTIVE.store(true, Ordering::Relaxed);
    crate::println!("mouse: VMware absolute pointer");
}

/// Whether PS/2 interrupts should be answered from the vmmouse queue.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Buttons held according to the last packet (bit 0 left, 1 right, 2
/// middle).
pub fn buttons() -> u8 {
    BUTTONS.load(Ordering::Relaxed)
}

/// Hand every queued packet to the mouse layer (from the PS/2
/// interrupt). Returns whether there were any.
pub fn poll() -> bool {
    let mut got = false;
    for _ in 0..64 {
        let st = cmd(CMD_ABSPOINTER_STATUS, 0);
        if st & STATUS_ERROR == STATUS_ERROR {
            // Queue overflow or a host-side reset: start over.
            crate::trace!("vmmouse: error status {st:#x}, re-enabling");
            cmd(CMD_ABSPOINTER_COMMAND, CMD_DISABLE);
            if !enable() {
                ACTIVE.store(false, Ordering::Relaxed);
                crate::println!("mouse: VMware absolute pointer lost, using PS/2");
            }
            return got;
        }
        if st & 0xffff < 4 {
            return got;
        }
        let Some(r) = vmware::command(CMD_ABSPOINTER_DATA, 4) else { return got };
        got = true;
        let flags = r.eax;
        let mut buttons = 0u8;
        if flags & LEFT != 0 {
            buttons |= 1;
        }
        if flags & RIGHT != 0 {
            buttons |= 2;
        }
        if flags & MIDDLE != 0 {
            buttons |= 4;
        }
        BUTTONS.store(buttons, Ordering::Relaxed);
        if flags & RELATIVE_PACKET != 0 {
            crate::mouse::motion(buttons, r.ebx as i32, r.ecx as i32);
        } else {
            crate::mouse::motion_abs(buttons, r.ebx.min(MAX), r.ecx.min(MAX), MAX, MAX);
        }
    }
    got
}
