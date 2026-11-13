//! Diskette drives on the PC's floppy disk controller (an Intel 82077AA
//! or compatible at 0x3F0, as QEMU's isa-fdc emulates it).
//!
//! Programmed I/O, polled, without DMA (the DOR's DMA gate stays off and
//! SPECIFY sets non-DMA mode): command bytes, data and result bytes all go
//! through the FIFO at 0x3F5 as the main status register asks for them;
//! seeks and recalibrations end when SENSE INTERRUPT STATUS reports them.
//! Interrupt 6 is never unmasked.
//!
//! Drives come from CMOS register 0x10 (high nibble A:, low nibble B:: 1 =
//! 360 KB, 2 = 1.2 MB, 3 = 720 KB, 4 = 1.44 MB, 5 = 2.88 MB). The medium's
//! format is sensed when it is first used after a change: the data rate
//! at which READ ID finds sector headers, then the highest sector number
//! track 0 has, then whether head 1 reads. The disk change line (DIR bit 7)
//! tells that the diskette was taken out; a seek away and back clears it
//! once a diskette is in. The motor is switched off two seconds after
//! the last access.
//!
//! The diskettes are drives A: and B: (`fs::removable`); the category 8
//! IOCtls (`api::ioctl`) reach the driver directly for the geometry,
//! formatting and the media sense.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::api::err;
use crate::arch::{inb, outb};
use crate::fs::block::BlockDev;
use crate::sync::{Mutex, SpinLock};

const BASE: u16 = 0x3f0;
const DOR: u16 = BASE + 2;
const MSR: u16 = BASE + 4;
const FIFO: u16 = BASE + 5;
/// Read: digital input (bit 7 disk change). Write: configuration control
/// (the data rate).
const DIR_CCR: u16 = BASE + 7;

const MSR_RQM: u8 = 0x80;
const MSR_DIO: u8 = 0x40;
const MSR_NONDMA: u8 = 0x20;

const DOR_NRESET: u8 = 0x04;

const CMD_SPECIFY: u8 = 0x03;
const CMD_RECALIBRATE: u8 = 0x07;
const CMD_SENSE_INT: u8 = 0x08;
const CMD_SEEK: u8 = 0x0f;
const CMD_VERSION: u8 = 0x10;
const CMD_WRITE: u8 = 0x45; // MFM
const CMD_READ: u8 = 0x46; // MFM
const CMD_READ_ID: u8 = 0x4a; // MFM
const CMD_FORMAT: u8 = 0x4d; // MFM

/// Data rates (CCR values).
pub const RATE_500K: u8 = 0;
pub const RATE_300K: u8 = 1;
pub const RATE_250K: u8 = 2;
pub const RATE_1M: u8 = 3;

/// Drive types (CMOS values).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    K360,
    K1200,
    K720,
    K1440,
    K2880,
}

impl Kind {
    fn from_cmos(n: u8) -> Option<Kind> {
        Some(match n {
            1 => Kind::K360,
            2 => Kind::K1200,
            3 => Kind::K720,
            4 => Kind::K1440,
            5 => Kind::K2880,
            _ => return None,
        })
    }

    /// The drive's own (highest capacity) format.
    pub fn native(self) -> Geom {
        match self {
            Kind::K360 => Geom { cyls: 40, heads: 2, spt: 9, rate: RATE_250K },
            Kind::K1200 => Geom { cyls: 80, heads: 2, spt: 15, rate: RATE_500K },
            Kind::K720 => Geom { cyls: 80, heads: 2, spt: 9, rate: RATE_250K },
            Kind::K1440 => Geom { cyls: 80, heads: 2, spt: 18, rate: RATE_500K },
            Kind::K2880 => Geom { cyls: 80, heads: 2, spt: 36, rate: RATE_1M },
        }
    }

    /// Data rates the drive reads, the most likely first.
    fn rates(self) -> &'static [u8] {
        match self {
            Kind::K360 | Kind::K720 => &[RATE_250K],
            Kind::K1200 => &[RATE_500K, RATE_300K],
            Kind::K1440 => &[RATE_500K, RATE_250K],
            Kind::K2880 => &[RATE_1M, RATE_500K, RATE_250K],
        }
    }
}

/// A diskette format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geom {
    pub cyls: u16,
    pub heads: u16,
    pub spt: u16,
    pub rate: u8,
}

impl Geom {
    pub fn sectors(&self) -> u64 {
        self.cyls as u64 * self.heads as u64 * self.spt as u64
    }

    /// The format of a medium of `sectors` sectors with `spt` sectors per
    /// track (a BPB for DSK_SETDEVICEPARAMS), if the drive can write it.
    pub fn from_bpb(kind: Kind, sectors: u32, spt: u16, heads: u16) -> Option<Geom> {
        let heads = heads.max(1);
        if spt == 0 || sectors == 0 {
            return None;
        }
        let cyls = (sectors / (spt as u32 * heads as u32)) as u16;
        let rate = match (spt, kind) {
            (36, Kind::K2880) => RATE_1M,
            (15..=21, Kind::K1200 | Kind::K1440 | Kind::K2880) => RATE_500K,
            (8 | 9, Kind::K1200) => RATE_300K,
            (8..=10, _) => RATE_250K,
            _ => return None,
        };
        (cyls > 0 && cyls <= 83).then_some(Geom { cyls, heads, spt, rate })
    }
}

/// The controller: one command at a time for both drives.
struct Fdc {
    /// Digital output register as last written (motor bits, selection).
    dor: u8,
    /// Data rate programmed.
    rate: u8,
    /// Cylinder each drive's head is over (None: recalibrate first).
    cyl: [Option<u8>; 2],
    /// When each motor was last needed (mono_ms).
    used: [u64; 2],
    /// Medium format per drive, None until sensed.
    media: [Option<Geom>; 2],
}

static FDC: Mutex<Fdc> = Mutex::new(Fdc { dor: DOR_NRESET, rate: 0xff, cyl: [None; 2], used: [0; 2], media: [None; 2] });

/// The controller does not end non-DMA writes (QEMU).
static STUCK_WRITES: AtomicBool = AtomicBool::new(false);

static DRIVES: SpinLock<[Option<Arc<Drive>>; 2]> = SpinLock::new([None, None]);

/// A diskette drive.
pub struct Drive {
    pub unit: u8,
    pub kind: Kind,
    /// Bumped whenever the medium changed (the change line was seen).
    generation: AtomicU32,
}

/// Drive `d` (0 = A:, 1 = B:), if it is a diskette drive.
pub fn unit(d: u8) -> Option<Arc<Drive>> {
    DRIVES.lock().get(d as usize).cloned().flatten()
}

/// Sleep once timer interrupts run; before (early boot) busy-wait.
fn delay_ms(ms: u64) {
    if crate::irq::ticks() > 0 && crate::task::sched::current_opt().is_some() {
        crate::task::sched::sleep_ms(ms);
    } else {
        wait_until(ms, || {}, || false);
    }
}

/// Wait up to `ms` milliseconds for `cond`, busy: by the time-stamp
/// counter clock, which runs with interrupts off (API calls), and a spin
/// count while it is not measured yet (early boot).
fn wait_until(ms: u64, mut pace: impl FnMut(), mut cond: impl FnMut() -> bool) -> bool {
    let end = crate::rtc::mono_ms() + ms;
    let mut n = 0u64;
    loop {
        if cond() {
            return true;
        }
        pace();
        n += 1;
        if crate::rtc::mono_ms() > end || n > ms * 20_000 {
            return cond();
        }
        core::hint::spin_loop();
    }
}

fn msr() -> u8 {
    unsafe { inb(MSR) }
}

/// Wait until the controller wants a byte (`dio` false) or has one.
/// Ok(true): as asked; Ok(false): it is in the other direction (the
/// result phase started early).
fn wait_rqm(dio: bool) -> Result<bool, u32> {
    if !wait_until(2000, || {}, || msr() & MSR_RQM != 0) {
        return Err(err::NOT_READY);
    }
    Ok((msr() & MSR_DIO != 0) == dio)
}

fn send(b: u8) -> Result<(), u32> {
    if !wait_rqm(false)? {
        return Err(30); // ERROR_READ_FAULT: the controller is out of step
    }
    unsafe { outb(FIFO, b) };
    Ok(())
}

fn recv() -> Result<u8, u32> {
    if !wait_rqm(true)? {
        return Err(30);
    }
    Ok(unsafe { inb(FIFO) })
}

fn command(bytes: &[u8]) -> Result<(), u32> {
    for &b in bytes {
        send(b)?;
    }
    Ok(())
}

/// The result phase: ST0, ST1, ST2, C, H, R, N.
fn result7() -> Result<[u8; 7], u32> {
    let mut r = [0u8; 7];
    for b in r.iter_mut() {
        *b = recv()?;
    }
    Ok(r)
}

/// SENSE INTERRUPT STATUS: (ST0, present cylinder); ST0 0x80 means no
/// interrupt was pending.
fn sense_int() -> Result<(u8, u8), u32> {
    send(CMD_SENSE_INT)?;
    let st0 = recv()?;
    if st0 == 0x80 {
        return Ok((st0, 0));
    }
    Ok((st0, recv()?))
}

/// Wait for the end of a seek or recalibration on `unit`.
fn wait_seek(unit: u8) -> Result<u8, u32> {
    let mut out = Err(err::NOT_READY);
    wait_until(3000, || {}, || match sense_int() {
        Ok((st0, pcn)) if st0 != 0x80 && st0 & 3 == unit && st0 & 0x20 != 0 => {
            out = if st0 & 0xd0 != 0 { Err(err::NOT_READY) } else { Ok(pcn) };
            true
        }
        Ok(_) => false,
        Err(e) => {
            out = Err(e);
            true
        }
    });
    out
}

/// Map a failed transfer's status bytes to an OS/2 error.
fn status_error(r: &[u8; 7], write: bool) -> u32 {
    let (st0, st1, st2) = (r[0], r[1], r[2]);
    if st0 & 0x08 != 0 {
        err::NOT_READY
    } else if st1 & 0x02 != 0 {
        err::WRITE_PROTECT
    } else if st1 & 0x20 != 0 || st2 & 0x20 != 0 {
        23 // ERROR_CRC
    } else if st1 & 0x85 != 0 || st2 & 0x01 != 0 {
        27 // ERROR_SECTOR_NOT_FOUND
    } else if write {
        29 // ERROR_WRITE_FAULT
    } else {
        30 // ERROR_READ_FAULT
    }
}

impl Fdc {
    fn out_dor(&mut self, v: u8) {
        self.dor = v;
        unsafe { outb(DOR, v) };
    }

    /// Reset the controller: non-DMA mode, step rate and head times for
    /// all drive types.
    fn reset(&mut self) -> Result<(), u32> {
        unsafe { outb(DOR, 0) };
        delay_ms(1);
        let keep = self.dor & 0x30;
        self.out_dor(DOR_NRESET | keep);
        // One interrupt status per drive the reset reached.
        for _ in 0..4 {
            sense_int()?;
        }
        self.set_rate(RATE_500K);
        // SRT 3 ms (at 500 kb/s), head unload 240 ms, head load 16 ms,
        // non-DMA.
        command(&[CMD_SPECIFY, 0xdf, (8 << 1) | 1])?;
        self.cyl = [None; 2];
        Ok(())
    }

    fn set_rate(&mut self, rate: u8) {
        if self.rate != rate {
            unsafe { outb(DIR_CCR, rate) };
            self.rate = rate;
        }
    }

    /// Select `unit` with its motor running.
    fn select(&mut self, unit: u8) {
        let motor = 0x10 << unit;
        let was_on = self.dor & motor != 0;
        self.out_dor((self.dor & 0x30) | motor | DOR_NRESET | unit);
        if !was_on {
            // Spin-up.
            delay_ms(300);
        }
        self.used[unit as usize] = crate::rtc::mono_ms();
    }

    /// Whether the change line of the (selected) drive is active.
    fn change_line(&self) -> bool {
        unsafe { inb(DIR_CCR) & 0x80 != 0 }
    }

    fn recalibrate(&mut self, unit: u8) -> Result<(), u32> {
        self.cyl[unit as usize] = None;
        // A recalibration steps at most 77 times on some controllers.
        for _ in 0..2 {
            command(&[CMD_RECALIBRATE, unit])?;
            if wait_seek(unit).is_ok() {
                self.cyl[unit as usize] = Some(0);
                return Ok(());
            }
        }
        Err(err::NOT_READY)
    }

    fn seek(&mut self, unit: u8, head: u8, cyl: u8) -> Result<(), u32> {
        if self.cyl[unit as usize].is_none() {
            self.recalibrate(unit)?;
        }
        if self.cyl[unit as usize] == Some(cyl) {
            return Ok(());
        }
        command(&[CMD_SEEK, (head << 2) | unit, cyl])?;
        match wait_seek(unit) {
            Ok(c) if c == cyl => {
                self.cyl[unit as usize] = Some(cyl);
                Ok(())
            }
            _ => {
                self.cyl[unit as usize] = None;
                Err(27)
            }
        }
    }

    /// READ ID on head `head` at the programmed rate: whether sector
    /// headers were found.
    fn read_id(&mut self, unit: u8, head: u8) -> Result<bool, u32> {
        command(&[CMD_READ_ID, (head << 2) | unit])?;
        // The controller looks for a header for up to two revolutions.
        if !wait_until(1500, || {}, || msr() & (MSR_RQM | MSR_DIO) == (MSR_RQM | MSR_DIO)) {
            // No index pulses: no diskette. Reset to end the command.
            self.reset()?;
            return Ok(false);
        }
        let r = result7()?;
        Ok(r[0] & 0xc0 == 0)
    }

    /// Read or write `count` sectors of one track from sector `sector`
    /// (1-based), with `buf` holding them.
    fn transfer(&mut self, unit: u8, g: &Geom, cyl: u8, head: u8, sector: u8, count: u8, buf: &mut [u8], write: bool) -> Result<(), u32> {
        self.select(unit);
        self.set_rate(g.rate);
        self.seek(unit, head, cyl)?;
        let eot = sector + count - 1;
        command(&[if write { CMD_WRITE } else { CMD_READ }, (head << 2) | unit, cyl, head, sector, 2, eot, gap(g), 0xff])?;
        let len = count as usize * 512;
        let mut i = 0;
        while i < len {
            // The data phase is over early when the controller turns to
            // its result (an error) or leaves non-DMA transfer mode.
            if !wait_until(2000, || {}, || msr() & MSR_RQM != 0) {
                self.reset()?;
                return Err(err::NOT_READY);
            }
            let s = msr();
            if s & MSR_NONDMA == 0 {
                break;
            }
            if write {
                if s & MSR_DIO != 0 {
                    break;
                }
                unsafe { outb(FIFO, buf[i]) };
            } else {
                if s & MSR_DIO == 0 {
                    break;
                }
                buf[i] = unsafe { inb(FIFO) };
            }
            i += 1;
        }
        // QEMU's controller writes the last sector of a non-DMA write but
        // never enters the result phase (it stops asking for data and
        // stays busy): when no result comes, the data went across; reset
        // the controller to end the command. Once seen, later writes do
        // not wait for it.
        if write && i == len {
            let have = if STUCK_WRITES.load(Ordering::Relaxed) {
                msr() & MSR_RQM != 0
            } else {
                wait_until(300, || {}, || msr() & MSR_RQM != 0)
            };
            if !have {
                STUCK_WRITES.store(true, Ordering::Relaxed);
                self.reset()?;
                return Ok(());
            }
        }
        let r = result7()?;
        // Without a terminal count the transfer ends at the end of the
        // track with "end of cylinder": that is success when all data
        // went across.
        let ok = r[0] & 0xc0 == 0 || (i == len && r[0] & 0xc0 == 0x40 && r[1] & !0x80 == 0 && r[2] == 0);
        if ok && i == len {
            Ok(())
        } else {
            Err(status_error(&r, write))
        }
    }

    /// Format track `cyl`/`head` with the sector headers `ids` (C, H, R,
    /// N), sectors of 128 << n bytes filled with 0xF6.
    fn format(&mut self, unit: u8, g: &Geom, cyl: u8, head: u8, n: u8, ids: &[[u8; 4]]) -> Result<(), u32> {
        self.select(unit);
        self.set_rate(g.rate);
        self.seek(unit, head, cyl)?;
        let gap3 = match g.spt {
            36 => 0x53,
            15..=21 => 0x54,
            _ => 0x50,
        };
        command(&[CMD_FORMAT, (head << 2) | unit, n, ids.len() as u8, gap3, 0xf6])?;
        // Four bytes per sector, unless the controller ends the command
        // without taking them (QEMU).
        'ids: for id in ids {
            for &b in id {
                if !wait_rqm(false)? {
                    break 'ids;
                }
                unsafe { outb(FIFO, b) };
            }
        }
        let r = result7()?;
        if r[0] & 0xc0 == 0 { Ok(()) } else { Err(status_error(&r, true)) }
    }

    /// Find the format of the diskette in `unit`.
    fn sense(&mut self, unit: u8, kind: Kind) -> Result<Geom, u32> {
        self.select(unit);
        self.recalibrate(unit)?;
        let mut found = None;
        for &rate in kind.rates() {
            self.set_rate(rate);
            if self.read_id(unit, 0)? {
                found = Some(rate);
                break;
            }
        }
        let rate = found.ok_or(err::NOT_READY)?;
        let spts: &[u16] = match rate {
            RATE_1M => &[36],
            RATE_500K => &[21, 18, 15],
            _ => &[10, 9, 8],
        };
        let mut buf = vec![0u8; 512];
        let cyls = if rate == RATE_300K || kind == Kind::K360 { 40 } else { 80 };
        let mut g = Geom { cyls, heads: 2, spt: 0, rate };
        for &spt in spts {
            let t = Geom { spt, ..g };
            if self.transfer(unit, &t, 0, 0, spt as u8, 1, &mut buf, false).is_ok() {
                g.spt = spt;
                break;
            }
        }
        if g.spt == 0 {
            return Err(27);
        }
        if self.transfer(unit, &g, 0, 1, 1, 1, &mut buf, false).is_err() {
            g.heads = 1;
        }
        Ok(g)
    }
}

/// GAP length for reads and writes.
fn gap(g: &Geom) -> u8 {
    match g.spt {
        36 => 0x1b,
        15..=21 => 0x1b,
        _ => 0x2a,
    }
}

impl Drive {
    /// Note a change of diskette if the change line says so (the format
    /// is sensed again); true if it changed.
    pub fn check_change(&self) -> bool {
        let mut f = FDC.lock();
        f.select(self.unit);
        if !f.change_line() {
            return false;
        }
        f.media[self.unit as usize] = None;
        self.generation.fetch_add(1, Ordering::Relaxed);
        // Moving the head clears the line once a diskette is in.
        let u = self.unit;
        let _ = f.recalibrate(u).and_then(|_| f.seek(u, 0, 1)).and_then(|_| f.seek(u, 0, 0));
        true
    }

    /// Changes of diskette seen so far.
    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Relaxed)
    }

    /// The format of the diskette now in the drive.
    pub fn media(&self) -> Result<Geom, u32> {
        let mut f = FDC.lock();
        if let Some(g) = f.media[self.unit as usize] {
            return Ok(g);
        }
        let r = f.sense(self.unit, self.kind);
        if let Ok(g) = r {
            crate::trace!("floppy {}: {} cylinders, {} heads, {} sectors per track", self.unit, g.cyls, g.heads, g.spt);
            f.media[self.unit as usize] = Some(g);
        }
        r
    }

    /// The format to read and write from now on (DSK_SETDEVICEPARAMS
    /// for a medium about to be formatted), or None to sense it again.
    pub fn set_media(&self, g: Option<Geom>) {
        FDC.lock().media[self.unit as usize] = g;
    }

    /// The medium's format if known, without touching the drive.
    pub fn known_media(&self) -> Option<Geom> {
        FDC.lock().media[self.unit as usize]
    }

    /// Format one track (DSK_FORMATVERIFY): `ids` are the sector headers.
    pub fn format_track(&self, cyl: u16, head: u16, n: u8, ids: &[[u8; 4]]) -> Result<(), u32> {
        let g = self.media_or_native();
        if cyl >= g.cyls || head >= g.heads {
            return Err(err::INVALID_PARAMETER);
        }
        let mut f = FDC.lock();
        let mut last = Ok(());
        for _ in 0..3 {
            last = f.format(self.unit, &g, cyl as u8, head as u8, n, ids);
            if last.is_ok() || last == Err(err::WRITE_PROTECT) {
                break;
            }
            let _ = f.recalibrate(self.unit);
        }
        last
    }

    /// The medium's format, or the drive's own for one that is not
    /// formatted yet.
    pub fn media_or_native(&self) -> Geom {
        self.media().unwrap_or_else(|_| {
            let g = self.kind.native();
            FDC.lock().media[self.unit as usize] = Some(g);
            g
        })
    }

    /// Read or write whole sectors from `lba` in `buf`.
    fn io(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let g = self.media()?;
        let n = (buf.len() / 512) as u64;
        if lba + n > g.sectors() {
            return Err(27);
        }
        let spt = g.spt as u64;
        let mut done = 0u64;
        while done < n {
            let at = lba + done;
            let (track, sector) = (at / spt, at % spt);
            let (cyl, head) = (track / g.heads as u64, track % g.heads as u64);
            let count = (spt - sector).min(n - done);
            let part = &mut buf[done as usize * 512..(done + count) as usize * 512];
            let mut f = FDC.lock();
            let mut last = Ok(());
            for attempt in 0..3 {
                last = f.transfer(self.unit, &g, cyl as u8, head as u8, sector as u8 + 1, count as u8, part, write);
                match last {
                    Ok(()) | Err(err::WRITE_PROTECT) => break,
                    Err(_) if attempt < 2 => {
                        if f.change_line() {
                            // The diskette went away.
                            f.media[self.unit as usize] = None;
                            self.generation.fetch_add(1, Ordering::Relaxed);
                            return Err(err::NOT_READY);
                        }
                        let _ = f.recalibrate(self.unit);
                    }
                    Err(_) => {}
                }
            }
            last?;
            done += count;
        }
        Ok(())
    }
}

impl BlockDev for Drive {
    fn sector_size(&self) -> usize {
        512
    }

    fn sector_count(&self) -> u64 {
        self.media().map(|g| g.sectors()).unwrap_or(0)
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.io(lba, buf, false)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut tmp = buf.to_vec();
        self.io(lba, &mut tmp, true)
    }
}

/// Switch motors off two seconds after their last use.
fn motor_thread() {
    loop {
        crate::task::sched::sleep_ms(1000);
        if let Some(mut f) = FDC.try_lock() {
            let now = crate::rtc::mono_ms();
            for u in 0..2u8 {
                let bit = 0x10 << u;
                if f.dor & bit != 0 && now.saturating_sub(f.used[u as usize]) > 2000 {
                    let v = f.dor & !bit;
                    f.out_dor(v);
                }
            }
        }
    }
}

/// Find the controller and its drives; returns the drives (A:, B:).
pub fn probe() -> Vec<Arc<Drive>> {
    let types = crate::rtc::cmos_read(0x10);
    if types == 0 {
        return Vec::new();
    }
    // Nothing answers at 0x3F4 without a controller.
    if msr() == 0xff {
        return Vec::new();
    }
    {
        let mut f = FDC.lock();
        if f.reset().is_err() {
            crate::println!("floppy: controller does not answer");
            return Vec::new();
        }
        let ver = command(&[CMD_VERSION]).and_then(|_| recv()).unwrap_or(0);
        crate::println!("floppy: controller version {ver:#x}");
    }
    let mut out = Vec::new();
    let mut d = DRIVES.lock();
    for unit in 0..2u8 {
        let n = if unit == 0 { types >> 4 } else { types & 0xf };
        if let Some(kind) = Kind::from_cmos(n) {
            crate::println!("floppy: drive {}: {:?}", (b'A' + unit) as char, kind);
            let dr = Arc::new(Drive { unit, kind, generation: AtomicU32::new(0) });
            d[unit as usize] = Some(dr.clone());
            out.push(dr);
        }
    }
    drop(d);
    if !out.is_empty() {
        crate::task::kthread::spawn("floppy motor", motor_thread);
    }
    out
}
