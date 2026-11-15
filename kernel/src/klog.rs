//! The kernel log kept in memory: everything that goes to the serial
//! port (log messages and traces) also goes into a ring buffer, for
//! machines without a serial port. The panic screen packs its tail into a
//! QR code (`crash.rs`), and with `bootlog` on the command line a kernel
//! thread copies it to `\RS2\BOOT.LOG` on the boot volume.
//!
//! Writers reserve their bytes with one atomic add, so CPUs never wait for
//! each other and a CPU that stopped mid-write (panics) leaves at most a
//! few garbled bytes.

use core::sync::atomic::{AtomicUsize, Ordering};

pub const SIZE: usize = 256 * 1024;

struct Ring(core::cell::UnsafeCell<[u8; SIZE]>);
// SAFETY: bytes are written at reserved positions; readers accept tearing.
unsafe impl Sync for Ring {}

static RING: Ring = Ring(core::cell::UnsafeCell::new([0; SIZE]));
/// Bytes written since boot.
static HEAD: AtomicUsize = AtomicUsize::new(0);

pub fn write(s: &[u8]) {
    let pos = HEAD.fetch_add(s.len(), Ordering::Relaxed);
    let ring = RING.0.get() as *mut u8;
    for (i, &b) in s.iter().enumerate() {
        unsafe { ring.add((pos + i) % SIZE).write_volatile(b) };
    }
}

/// Bytes written since boot (a position for [`since`]).
pub fn position() -> usize {
    HEAD.load(Ordering::Relaxed)
}

/// Copy what was written from `pos` on (at most the ring's size, and the
/// last `out.len()` bytes of that) into `out`; the length.
pub fn since(pos: usize, out: &mut [u8]) -> usize {
    let head = HEAD.load(Ordering::Relaxed);
    let start = pos.max(head.saturating_sub(SIZE)).max(head.saturating_sub(out.len()));
    let ring = RING.0.get() as *const u8;
    for (i, p) in (start..head).enumerate() {
        out[i] = unsafe { ring.add(p % SIZE).read_volatile() };
    }
    head - start
}

/// The last `out.len()` bytes (or all there is).
pub fn tail(out: &mut [u8]) -> usize {
    since(0, out)
}

/// Set once `boot::start` has returned (CONFIG.SYS read, the shell
/// started).
pub static BOOTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// `bootlog`: copy the log to `\RS2\BOOT.LOG` on the boot volume every
/// few seconds while it grows (the first copy once the disks are up).
pub fn start_bootlog() {
    if !crate::boot_info().cmdline().split_whitespace().any(|w| w.eq_ignore_ascii_case("bootlog")) {
        return;
    }
    crate::task::kthread::spawn("bootlog", || {
        let mut written = usize::MAX;
        let mut buf = alloc::vec![0u8; SIZE];
        let mut warned = false;
        // Not while boot::start runs: it does its file I/O on CPU 0's idle
        // thread, and disk I/O from here contending with it corrupted the
        // idle thread's stack (a kernel trap in schedule() right after
        // "config.sys:", 2026-11-15; docs/SURFACE-PRO-4.md, phase 0).
        while !BOOTED.load(Ordering::Acquire) {
            crate::task::sched::sleep_ms(200);
        }
        loop {
            let head = position();
            if head != written {
                match boot_volume() {
                    Some(fs) => {
                        let n = tail(&mut buf);
                        if save(&*fs, "\\RS2\\BOOT.LOG", &buf[..n]).is_ok() {
                            written = head;
                        }
                    }
                    None if !warned => {
                        warned = true;
                        crate::println!("bootlog: no writable boot volume (booted from a CD?)");
                    }
                    None => {}
                }
            }
            crate::task::sched::sleep_ms(5000);
        }
    });
}

/// The volume the machine booted from, if it is a writable disk: the drive
/// holding `\RS2\KERNEL` that is not the loader's RAM image (C.IMG), a
/// CD or a network drive.
pub fn boot_volume() -> Option<alloc::sync::Arc<dyn crate::fs::FileSystem>> {
    let ram_c = crate::fs::has_boot_image(crate::boot_info());
    (2u8..26).filter(|&d| !(d == 2 && ram_c)).filter_map(crate::fs::drive).find(|fs| matches!(fs.name(), "FAT" | "HPFS" | "JFS") && fs.stat("\\RS2\\KERNEL").is_ok())
}

fn save(fs: &dyn crate::fs::FileSystem, path: &str, data: &[u8]) -> Result<(), u32> {
    let f = match fs.open(path) {
        Ok(f) => f,
        Err(_) => fs.create(path, 0)?,
    };
    f.write(0, data)?;
    f.set_size(data.len() as u64)?;
    f.flush()
}
