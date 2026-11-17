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

/// `bootlog`: copy the log to `\RS2\BOOT.LOG` on the boot volume every
/// few seconds while it grows (the first copy once the disks are up); the
/// previous boot's becomes `\RS2\BOOTPREV.LOG`.
pub fn start_bootlog() {
    if !crate::boot_info().cmdline().split_whitespace().any(|w| w.eq_ignore_ascii_case("bootlog")) {
        return;
    }
    crate::task::kthread::spawn("bootlog", || {
        let mut written = usize::MAX;
        let mut buf = alloc::vec![0u8; SIZE];
        let mut warned = false;
        let mut waiting = false;
        // Saves to the boot volume that failed in a row: a live boot's
        // stick is on USB, which may be what went wrong (the Surface Dock
        // stopped all USB).
        let mut failures = 0u32;
        let mut fell_back = false;
        let mut save_failed = false;
        loop {
            let head = position();
            if head != written {
                let target = match boot_volume() {
                    Some(v) if failures < 3 => Some((v, "RS2\\BOOT.LOG", "RS2\\BOOTPREV.LOG")),
                    // A live boot without its stick: another rs2 EFI
                    // partition (the internal disk's), under a name of its
                    // own, so the installed system's log stays.
                    _ => live_fallback().map(|v| (v, "RS2\\LIVEBOOT.LOG", "RS2\\LIVEPREV.LOG")),
                };
                match target {
                    // Not to a volume that was not shut down cleanly until
                    // CHKDSK /F (AUTOCHECK) has been over it.
                    Some(((d, _), ..)) if crate::fs::unchecked(d) => {
                        if !waiting {
                            waiting = true;
                            crate::println!("bootlog: {}: was not shut down cleanly; no BOOT.LOG until CHKDSK has checked it", (b'A' + d) as char);
                        }
                    }
                    Some(((d, fs), name, prev)) => {
                        let fallback = name.contains("LIVE");
                        if fallback && !fell_back {
                            fell_back = true;
                            written = usize::MAX;
                            crate::println!("bootlog: the boot stick cannot be written; the log goes to {}:\\{name}", (b'A' + d) as char);
                        }
                        if written == usize::MAX {
                            // The last boot's log stays as the previous one
                            // (read it after a boot that went wrong).
                            let _ = fs.delete(prev);
                            let _ = fs.rename(name, prev);
                        }
                        let path = alloc::format!("\\{name}");
                        let n = tail(&mut buf);
                        match save(&*fs, &path, &buf[..n]) {
                            Ok(()) => {
                                written = head;
                                if !fallback {
                                    failures = 0;
                                }
                            }
                            Err(e) => {
                                // Say why, once, and keep the newest lines in
                                // the room the file already has (a full
                                // volume cannot extend it).
                                if !save_failed {
                                    save_failed = true;
                                    let free = fs.volume().map_or(0, |v| v.free_units as u64 * v.sectors_per_unit as u64 * v.bytes_per_sector as u64 / 1024);
                                    crate::println!("bootlog: saving {}:{path} failed (error {e}, {free} KB free); keeping the newest lines that fit", (b'A' + d) as char);
                                }
                                let room = fs.stat(&path).map_or(0, |i| i.alloc.max(i.size)) as usize;
                                let n = tail(&mut buf);
                                if room > 0 && save(&*fs, &path, &buf[n.saturating_sub(room)..n]).is_ok() {
                                    written = head;
                                }
                                if !fallback {
                                    failures += 1;
                                }
                            }
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

/// On a live boot (C: the RAM disk): the first other drive holding an rs2
/// kernel, not the boot stick. None otherwise.
fn live_fallback() -> Option<(u8, alloc::sync::Arc<dyn crate::fs::FileSystem>)> {
    if !crate::fs::has_boot_image(crate::boot_info()) {
        return None;
    }
    let b = crate::drivers::BOOT_DRIVE.load(core::sync::atomic::Ordering::Relaxed);
    (3u8..26).filter(|&d| d != b).filter_map(|d| Some((d, crate::fs::drive(d)?))).find(|(_, fs)| matches!(fs.name(), "FAT" | "HPFS" | "JFS") && fs.stat("\\RS2\\KERNEL").is_ok())
}

/// `bootdiag`: if no PM program has the display 60 s after boot (and
/// the boot is not to a command prompt), log every thread's state, the
/// Ctrl+Alt+F12 report, once: BOOT.LOG then shows what boot waits for on
/// a machine without a serial port.
pub fn start_boot_watch() {
    let cmd = crate::boot_info().cmdline();
    if !bootinfo::diag_on(cmd) || cmd.split_whitespace().any(|w| w.eq_ignore_ascii_case("textmode") || w.starts_with("init=")) {
        return;
    }
    crate::task::kthread::spawn("bootwatch", || {
        crate::task::sched::sleep_ms(60_000);
        if crate::display::pm_started() {
            return;
        }
        crate::println!("bootdiag: no desktop 60 s after boot; every thread:");
        for l in crate::debug::thread_report().lines() {
            crate::println!("{l}");
        }
    });
}

/// The volume the machine booted from, if it is a writable disk (drive
/// number and file system): the drive holding `\RS2\KERNEL` that is not
/// the loader's RAM image (C.IMG), a CD or a network drive.
pub fn boot_volume() -> Option<(u8, alloc::sync::Arc<dyn crate::fs::FileSystem>)> {
    let usable = |fs: &alloc::sync::Arc<dyn crate::fs::FileSystem>| matches!(fs.name(), "FAT" | "HPFS" | "JFS") && fs.stat("\\RS2\\KERNEL").is_ok();
    // The partition the firmware booted from, when the drivers know it.
    let b = crate::drivers::BOOT_DRIVE.load(core::sync::atomic::Ordering::Relaxed);
    if b != 0 {
        return crate::fs::drive(b).filter(usable).map(|fs| (b, fs));
    }
    // Not known: on a live boot (C: the RAM disk) write nowhere rather than
    // to another system's EFI partition (the installed system's log).
    let ram_c = crate::fs::has_boot_image(crate::boot_info());
    if ram_c {
        return None;
    }
    (2u8..26).filter_map(|d| Some((d, crate::fs::drive(d)?))).find(|(_, fs)| usable(fs))
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
