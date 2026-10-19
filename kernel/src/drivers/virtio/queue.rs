//! Split virtqueues: a descriptor table, the available ring the driver
//! fills and the used ring the device returns buffers on.
//!
//! The three parts sit in one physically contiguous block in the legacy
//! layout (descriptors, available ring, then the used ring at the next
//! 4 KB boundary), which suits both transports. Buffers are chained
//! descriptors; neither indirect descriptors nor event indices are
//! negotiated, and the device is asked not to interrupt.

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use crate::arch::{inb, outw};
use crate::mm::phys;

const PAGE: usize = 4096;

// Descriptor flags.
const DESC_NEXT: u16 = 1;
const DESC_WRITE: u16 = 2;
/// Available ring flag: no interrupts wanted.
const AVAIL_NO_INTERRUPT: u16 = 1;
/// Longest descriptor chain a request uses.
const MAX_CHAIN: usize = 8;
/// Used ring flag: the device does not need notifications.
const USED_NO_NOTIFY: u16 = 1;

/// A device register: memory-mapped or an I/O port.
#[derive(Clone, Copy)]
pub enum Reg {
    Mmio(*mut u8),
    Port(u16),
}

impl Reg {
    pub fn read8(self) -> u8 {
        match self {
            Reg::Mmio(p) => unsafe { read_volatile(p) },
            Reg::Port(p) => unsafe { inb(p) },
        }
    }

    fn write16(self, v: u16) {
        match self {
            Reg::Mmio(p) => unsafe { write_volatile(p as *mut u16, v) },
            Reg::Port(p) => unsafe { outw(p, v) },
        }
    }
}

/// One buffer of a request: physical address, length, and whether the
/// device writes it (else it reads it).
#[derive(Clone, Copy)]
pub struct Buf {
    pub addr: u64,
    pub len: u32,
    pub device_writes: bool,
}

impl Buf {
    pub fn out(addr: u64, len: usize) -> Buf {
        Buf { addr, len: len as u32, device_writes: false }
    }

    pub fn into(addr: u64, len: usize) -> Buf {
        Buf { addr, len: len as u32, device_writes: true }
    }
}

/// A buffer chain the device has finished with.
#[derive(Clone, Copy, Debug)]
pub struct Used {
    pub head: u16,
    /// Bytes the device wrote.
    pub len: u32,
    /// Address of the first buffer of the chain.
    pub addr: u64,
}

pub struct Virtqueue {
    pub index: u16,
    size: u16,
    /// Physical base and page count of the ring memory.
    base: u64,
    pages: usize,
    avail_off: usize,
    used_off: usize,
    free: Vec<u16>,
    next_avail: u16,
    last_used: u16,
    notify: Reg,
    /// A register read between polls (see `virtio::wait`).
    pace: Reg,
}

unsafe impl Send for Virtqueue {}

impl Virtqueue {
    /// Allocate a queue of `size` entries (a power of two).
    pub fn new(index: u16, size: u16, notify: Reg, pace: Reg) -> Option<Virtqueue> {
        let n = size as usize;
        let avail_off = 16 * n;
        let used_off = (avail_off + 6 + 2 * n).next_multiple_of(PAGE);
        let pages = (used_off + 6 + 8 * n).div_ceil(PAGE);
        let base = phys::alloc_contig(pages as u64, 1 << 32)?;
        let q = Virtqueue {
            index,
            size,
            base,
            pages,
            avail_off,
            used_off,
            free: (0..size).rev().collect(),
            next_avail: 0,
            last_used: 0,
            notify,
            pace,
        };
        unsafe { write_volatile(q.ptr(avail_off) as *mut u16, AVAIL_NO_INTERRUPT) };
        Some(q)
    }

    pub fn with_notify(mut self, notify: Reg) -> Virtqueue {
        self.notify = notify;
        self
    }

    pub fn size(&self) -> u16 {
        self.size
    }

    pub fn desc_addr(&self) -> u64 {
        self.base
    }

    pub fn avail_addr(&self) -> u64 {
        self.base + self.avail_off as u64
    }

    pub fn used_addr(&self) -> u64 {
        self.base + self.used_off as u64
    }

    fn ptr(&self, off: usize) -> *mut u8 {
        phys::phys_to_virt(self.base + off as u64)
    }

    fn desc(&self, i: u16) -> *mut u8 {
        self.ptr(i as usize * 16)
    }

    pub fn free_count(&self) -> usize {
        self.free.len()
    }

    /// Put a chain of buffers on the available ring (the device sees it
    /// after [`Virtqueue::kick`]). Returns the head descriptor, or None if
    /// the queue is full.
    pub fn add(&mut self, bufs: &[Buf]) -> Option<u16> {
        // No allocation here: input queues are refilled from the timer tick.
        let mut ids = [0u16; MAX_CHAIN];
        if bufs.is_empty() || bufs.len() > MAX_CHAIN || bufs.len() > self.free.len() {
            return None;
        }
        for id in ids.iter_mut().take(bufs.len()) {
            *id = self.free.pop().unwrap();
        }
        for (i, b) in bufs.iter().enumerate() {
            let d = self.desc(ids[i]);
            let mut flags = if b.device_writes { DESC_WRITE } else { 0 };
            let next = if i + 1 < bufs.len() {
                flags |= DESC_NEXT;
                ids[i + 1]
            } else {
                0
            };
            unsafe {
                write_volatile(d as *mut u64, b.addr);
                write_volatile(d.add(8) as *mut u32, b.len);
                write_volatile(d.add(12) as *mut u16, flags);
                write_volatile(d.add(14) as *mut u16, next);
            }
        }
        let head = ids[0];
        let slot = self.avail_off + 4 + 2 * (self.next_avail % self.size) as usize;
        unsafe { write_volatile(self.ptr(slot) as *mut u16, head) };
        self.next_avail = self.next_avail.wrapping_add(1);
        // The ring entry before the index that publishes it.
        fence(Ordering::SeqCst);
        unsafe { write_volatile(self.ptr(self.avail_off + 2) as *mut u16, self.next_avail) };
        Some(head)
    }

    /// Tell the device about new buffers (unless it said it polls).
    pub fn kick(&self) {
        fence(Ordering::SeqCst);
        let flags = unsafe { read_volatile(self.ptr(self.used_off) as *const u16) };
        if flags & USED_NO_NOTIFY == 0 {
            self.notify.write16(self.index);
        }
    }

    /// The next chain the device has returned, if any; its descriptors are
    /// free again.
    pub fn pop(&mut self) -> Option<Used> {
        let idx = unsafe { read_volatile(self.ptr(self.used_off + 2) as *const u16) };
        if idx == self.last_used {
            return None;
        }
        fence(Ordering::SeqCst);
        let e = self.used_off + 4 + 8 * (self.last_used % self.size) as usize;
        let (id, len) = unsafe { (read_volatile(self.ptr(e) as *const u32), read_volatile(self.ptr(e + 4) as *const u32)) };
        self.last_used = self.last_used.wrapping_add(1);
        let head = id as u16;
        if head >= self.size {
            return None;
        }
        let addr = unsafe { read_volatile(self.desc(head) as *const u64) };
        // Free the chain.
        let mut i = head;
        for _ in 0..self.size {
            self.free.push(i);
            let d = self.desc(i);
            let flags = unsafe { read_volatile(d.add(12) as *const u16) };
            if flags & DESC_NEXT == 0 {
                break;
            }
            i = unsafe { read_volatile(d.add(14) as *const u16) };
        }
        Some(Used { head, len, addr })
    }

    /// Submit a chain and wait up to `ms` for it to come back (one request
    /// in flight at a time). Returns the bytes the device wrote.
    pub fn call(&mut self, bufs: &[Buf], ms: u64) -> Option<u32> {
        let head = self.add(bufs)?;
        self.kick();
        let pace = self.pace;
        let mut got = None;
        let done = super::wait(
            ms,
            || {
                pace.read8();
            },
            || {
                while let Some(u) = self.pop() {
                    if u.head == head {
                        got = Some(u.len);
                    }
                }
                got.is_some()
            },
        );
        if !done {
            return None;
        }
        got
    }
}

impl Drop for Virtqueue {
    fn drop(&mut self) {
        for i in 0..self.pages {
            phys::free(self.base + (i * PAGE) as u64);
        }
    }
}

/// Physically contiguous, zeroed DMA memory below 4 GB, freed on drop.
pub struct Dma {
    pub base: u64,
    pages: usize,
}

unsafe impl Send for Dma {}

impl Dma {
    pub fn new(bytes: usize) -> Option<Dma> {
        let pages = bytes.div_ceil(PAGE).max(1);
        Some(Dma { base: phys::alloc_contig(pages as u64, 1 << 32)?, pages })
    }

    pub fn ptr(&self, off: usize) -> *mut u8 {
        phys::phys_to_virt(self.base + off as u64)
    }

    pub fn addr(&self, off: usize) -> u64 {
        self.base + off as u64
    }

    pub fn bytes(&self, off: usize, len: usize) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr(off), len) }
    }

    pub fn write(&self, off: usize, data: &[u8]) {
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr(off), data.len()) };
    }
}

impl Drop for Dma {
    fn drop(&mut self) {
        for i in 0..self.pages {
            phys::free(self.base + (i * PAGE) as u64);
        }
    }
}
